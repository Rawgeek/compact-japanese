//! Fixed Modified Hepburn romanization, with compact precomputed
//! dictionary readings. No token interning or address-history cache.
#![deny(unsafe_op_in_unsafe_fn)]

mod codec;
mod index;
mod mapping;
mod matrix;
mod tokenizer;

use std::borrow::Cow;
use std::cell::RefCell;
use std::ffi::{CStr, c_char};
use std::sync::Arc;
use tokenizer::{Engine, Parser};

/// A bounded conversion failed, or the immutable dictionary is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Dictionary,
    InvalidInput,
    Conversion,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Dictionary => "invalid or unavailable dictionary",
            Self::InvalidInput => "input contains NUL or exceeds 16 KiB",
            Self::Conversion => "reading could not be romanized",
        })
    }
}
impl std::error::Error for Error {}

/// Share one immutable mapping across threads. Scratch storage is thread local
/// and bounded; input text and output strings are never retained between calls.
#[derive(Clone)]
pub struct Romanizer {
    engine: Arc<Engine>,
}
impl Romanizer {
    pub fn open(dictionary: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        Engine::open(dictionary.as_ref())
            .map(|engine| Self { engine })
            .map_err(|_| Error::Dictionary)
    }
    pub fn romanize(&self, text: &str) -> Result<String, Error> {
        if text.len() > MAX_INPUT || text.contains('\0') {
            return Err(Error::InvalidInput);
        }
        romanize(Arc::clone(&self.engine), text).map_err(|_| Error::Conversion)
    }
}

/// Build the lossless byte index; verify every source state and terminal.
pub fn compile_index(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(usize, usize, usize), String> {
    index::compile(source, destination)
}
/// Deduplicate equal contexts, encode tiles and verify every retained score.
pub fn compile_matrix(
    source: &std::path::Path,
    destination: &std::path::Path,
    maps: &std::path::Path,
) -> Result<(usize, usize, usize), String> {
    matrix::compile(source, destination, maps, 5, 5)
}

const OK: i32 = 0;
const INVALID: i32 = 1;
const CAPACITY: i32 = 2;
const FAILED: i32 = 3;
// Shipping fields are far smaller. This bounds retained lattice scratch even
// if an accidental caller supplies an entire document. Python preserves input
// on failure, exactly as it does for other romanization errors.
const MAX_INPUT: usize = 16 * 1024;
const KIND: u8 = 7;
const PUNCT: u8 = 1;
const PREFIX: u8 = 2;
const SUFFIX: u8 = 3;
const VERB: u8 = 4;
const AUX: u8 = 5;
const ADJECTIVE: u8 = 6;
const PROPER: u8 = 8;
const CONNECTING: u8 = 16;
const SURFACE: u8 = 32;
const UNMAPPED: u8 = 64;

thread_local! {
    static PARSER: RefCell<Option<Parser>> = const { RefCell::new(None) };
}

fn hiragana(c: char) -> char {
    if ('\u{30a1}'..='\u{30f6}').contains(&c) || c == 'ヽ' || c == 'ヾ' {
        char::from_u32(c as u32 - 0x60).unwrap()
    } else {
        c
    }
}

fn kana(text: &str) -> Result<String, ()> {
    let chars: Vec<char> = text.chars().map(hiragana).collect();
    let mut output = String::with_capacity(text.len());
    for (i, &current) in chars.iter().enumerate() {
        let previous = i.checked_sub(1).map(|j| chars[j]);
        let next = chars.get(i + 1).copied();
        if "々〃ゝゞヽ".contains(current) {
            if current == 'ゝ' || current == 'ヽ' {
                if let Some(p) = previous {
                    output.push(p);
                }
            } else if current == 'ゞ'
                && let Some(p) = previous
                && let Some(index) = "かきくけこさしすせそたちつてとはひふへほ"
                    .chars()
                    .position(|c| c == p)
            {
                let voiced = "がぎぐげござじずぜぞだぢづでどばびぶべぼ"
                    .chars()
                    .nth(index)
                    .ok_or(())?;
                output.push_str(mapping::single(voiced).ok_or(())?);
            }
            continue;
        }
        if let Some(mapped) = previous.and_then(|p| mapping::pair(p, current)) {
            output.push_str(mapped);
            continue;
        }
        if next.is_some_and(|n| mapping::pair(current, n).is_some()) {
            continue;
        }
        if next.is_some_and(|n| "ゃゅょぁぃぅぇぉ".contains(n)) {
            if current != 'っ' {
                let base = mapping::single(current).ok_or(())?;
                output.push_str(&base[..base.len().saturating_sub(1)]);
                output.push_str(mapping::single(next.unwrap()).ok_or(())?);
            }
        } else if "ゃゅょぁぃぅぇぉ".contains(current) {
            continue;
        } else if current == 'ー' {
            if let Some(last) = previous
                .and_then(mapping::single)
                .and_then(|s| s.chars().last())
            {
                output.push(last);
            } else {
                output.push('-');
            }
        } else if current == 'っ' {
            match next {
                Some('ち') => output.push('t'),
                Some(n) if "あいうえおっ".contains(n) => output.push('-'),
                Some(n) => output.push(
                    mapping::single(n)
                        .and_then(|s| s.chars().next())
                        .ok_or(())?,
                ),
                None => output.push('っ'),
            }
        } else if current == 'ん' {
            output.push_str(if next.is_some_and(|n| "あいうえおやゆよ".contains(n)) {
                "n'"
            } else {
                "n"
            });
        } else {
            output.push_str(mapping::single(current).ok_or(())?);
        }
    }
    Ok(output)
}

fn title(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cased = false;
    for c in text.chars() {
        if cased {
            out.extend(c.to_lowercase());
        } else {
            out.extend(c.to_uppercase());
        }
        cased = c.is_lowercase() || c.is_uppercase();
    }
    out
}

struct Word<'a> {
    surface: &'a str,
    reading: &'a str,
    flags: u8,
    unknown: bool,
    char_type: u8,
    whitespace: bool,
}
struct Token<'a> {
    text: Cow<'a, str>,
    space: bool,
}

fn word_romaji<'a>(word: &Word<'a>) -> Result<Cow<'a, str>, ()> {
    let exception = match word.surface {
        "ホッピー" => Some("Hoppy"),
        "東京" => Some("Tokyo"),
        "大阪" => Some("Osaka"),
        "京都" => Some("Kyoto"),
        "柔道" => Some("judo"),
        "弁当" => Some("bento"),
        "ヒルズ" => Some("hills"),
        "カービィ" => Some("Kirby"),
        _ => None,
    };
    if let Some(value) = exception {
        return Ok(Cow::Borrowed(value));
    }
    if word.surface.is_ascii() {
        return Ok(Cow::Borrowed(word.surface));
    }
    if word.unknown {
        return Ok(Cow::Owned(if word.char_type == 6 || word.char_type == 7 {
            kana(word.surface)?
        } else {
            "?".repeat(word.surface.chars().count())
        }));
    }
    if word.flags & KIND == PUNCT {
        let mut chars = word.surface.chars();
        let mapped = match (chars.next(), chars.next(), chars.next()) {
            (Some(a), None, None) => mapping::single(a),
            (Some(a), Some(b), None) => mapping::pair(a, b),
            _ => None,
        }
        .unwrap_or("");
        return Ok(Cow::Borrowed(mapped));
    }
    if word.flags & UNMAPPED != 0 {
        return Err(());
    }
    Ok(Cow::Borrowed(if word.flags & SURFACE != 0 {
        word.surface
    } else {
        word.reading
    }))
}

fn render(words: &[Word<'_>]) -> Result<String, ()> {
    let mut tokens: Vec<Token<'_>> = Vec::with_capacity(words.len());
    for (i, word) in words.iter().enumerate() {
        let next = words.get(i + 1);
        if word.surface == "'"
            && !word.whitespace
            && next.is_some_and(|n| n.char_type == 5 && !n.whitespace)
        {
            if let Some(previous) = tokens.last_mut() {
                previous.space = false;
            }
            tokens.push(Token {
                text: Cow::Borrowed("'"),
                space: false,
            });
            continue;
        }
        let mut roma = word_romaji(word)?;
        if !roma.is_empty()
            && let Some(previous) = tokens.last_mut().filter(|t| t.text.ends_with('っ'))
        {
            let text = previous.text.to_mut();
            text.truncate(text.len() - 'っ'.len_utf8());
            text.push(roma.chars().next().ok_or(())?);
        }
        if word.flags & PROPER != 0 {
            roma = Cow::Owned(title(&roma));
        }
        let special = "「『".contains(word.surface) || "([".contains(roma.as_ref());
        if special {
            if let Some(previous) = tokens.last_mut() {
                previous.space = true;
            }
            tokens.push(Token {
                text: roma,
                space: false,
            });
            continue;
        }
        if roma == "/" {
            tokens.push(Token {
                text: roma,
                space: false,
            });
            continue;
        }
        let space = word.flags & KIND != PREFIX
            && !next.is_some_and(|n| {
                matches!(n.flags & KIND, PUNCT | SUFFIX)
                    || n.surface == ","
                    || n.flags & CONNECTING != 0
                    || word.surface.bytes().all(|c| c.is_ascii_digit())
                        && n.surface.bytes().all(|c| c.is_ascii_digit())
                    || matches!(word.flags & KIND, VERB | AUX | ADJECTIVE)
                        && n.flags & KIND == AUX
                        && n.surface != "です"
            });
        tokens.push(Token { text: roma, space });
    }
    for token in &mut tokens {
        if token.text.contains('っ') {
            token.text = Cow::Owned(token.text.replace('っ', ""));
        }
    }
    if let Some(first) = tokens.first_mut().filter(|t| !t.text.is_empty()) {
        let c = first.text.chars().next().ok_or(())?;
        let upper: String = c.to_uppercase().collect();
        if upper != c.to_string() {
            first.text = Cow::Owned(format!("{upper}{}", &first.text[c.len_utf8()..]));
        }
    }
    let mut output = String::with_capacity(words.iter().map(|w| w.surface.len()).sum());
    for token in tokens {
        output.push_str(&token.text);
        if token.space {
            output.push(' ');
        }
    }
    Ok(output.trim().to_owned())
}

fn romanize(engine: Arc<Engine>, text: &str) -> Result<String, ()> {
    // These are the only ASCII folds in Mojimoji 0.0.13 that survive NFKC.
    // Keep the raw core compatible without retaining that Python extension.
    let normalized: Cow<'_, str> = if text.chars().any(|c| matches!(c, '‘' | '’' | '”')) {
        Cow::Owned(
            text.chars()
                .map(|c| match c {
                    '‘' => '`',
                    '’' => '\'',
                    '”' => '"',
                    _ => c,
                })
                .collect(),
        )
    } else {
        Cow::Borrowed(text)
    };
    let text = normalized.as_ref();
    PARSER.with(|slot| {
        let mut slot = slot.try_borrow_mut().map_err(|_| ())?;
        if slot
            .as_ref()
            .is_none_or(|p| !Arc::ptr_eq(&p.engine, &engine))
        {
            *slot = Some(Parser::new(engine)?);
        }
        let parser = slot.as_mut().ok_or(())?;
        let result = (|| {
            let engine = Arc::clone(&parser.engine);
            let parsed = parser.analyze(text)?;
            let mut words = Vec::with_capacity(parsed.len());
            for current in parsed {
                let (flags, reading) = engine.feature(current.feature)?;
                words.push(Word {
                    surface: text.get(current.start..current.end).ok_or(())?,
                    reading,
                    flags,
                    unknown: current.unknown,
                    char_type: current.kind,
                    whitespace: current.whitespace,
                });
            }
            render(&words)
        })();
        parser.finish();
        result
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn cj_abi_version() -> u32 {
    2
}

/// # Safety
/// The path is a live NUL-terminated UTF-8 string. The returned owner must
/// remain live during calls and be released exactly once with cj_close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cj_open(dictionary: *const c_char) -> *const std::ffi::c_void {
    if dictionary.is_null() {
        return std::ptr::null();
    }
    std::panic::catch_unwind(|| {
        let dictionary = unsafe { CStr::from_ptr(dictionary) }
            .to_str()
            .map_err(|_| ())?;
        Engine::open(std::path::Path::new(dictionary)).map(|engine| Arc::into_raw(engine).cast())
    })
    .ok()
    .and_then(Result::ok)
    .unwrap_or(std::ptr::null())
}

/// # Safety
/// context is a live, unclosed owner returned by cj_open. No calls may race
/// closing that owner. Thread-local parsing state retains its own Arc.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cj_close(context: *const std::ffi::c_void) {
    if !context.is_null() {
        drop(unsafe { Arc::from_raw(context.cast::<Engine>()) });
    }
}

unsafe fn input<'a>(data: *const u8, len: usize) -> Result<&'a str, ()> {
    if len > MAX_INPUT || (data.is_null() && len != 0) {
        return Err(());
    }
    let bytes = if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    if text.contains('\0') {
        return Err(());
    }
    Ok(text)
}

unsafe fn output(value: &str, buffer: *mut u8, capacity: usize, required: *mut usize) -> i32 {
    if required.is_null() {
        return INVALID;
    }
    unsafe {
        *required = value.len();
    }
    if capacity < value.len() {
        return CAPACITY;
    }
    if !value.is_empty() {
        if buffer.is_null() {
            return INVALID;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), buffer, value.len());
        }
    }
    OK
}

/// # Safety
/// Input/output buffers are live, nonoverlapping allocations of their stated
/// lengths; required is writable. context follows cj_open's ownership contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cj_romanize(
    context: *const std::ffi::c_void,
    data: *const u8,
    len: usize,
    buffer: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> i32 {
    if context.is_null() || required.is_null() {
        return INVALID;
    }
    let Ok(text) = (unsafe { input(data, len) }) else {
        return INVALID;
    };
    std::panic::catch_unwind(|| {
        unsafe {
            Arc::increment_strong_count(context.cast::<Engine>());
        }
        let engine = unsafe { Arc::from_raw(context.cast::<Engine>()) };
        match romanize(engine, text) {
            Ok(value) => unsafe { output(&value, buffer, capacity, required) },
            Err(()) => FAILED,
        }
    })
    .unwrap_or(FAILED)
}

/// Build-time reading compiler. It shares the exact kana rules used for
/// unknown kana at runtime, without loading a model or dictionary.
/// # Safety
/// Buffers follow cj_romanize's allocation and nonoverlap contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cj_compile_reading(
    data: *const u8,
    len: usize,
    buffer: *mut u8,
    capacity: usize,
    required: *mut usize,
) -> i32 {
    let Ok(text) = (unsafe { input(data, len) }) else {
        return INVALID;
    };
    std::panic::catch_unwind(|| match kana(text) {
        Ok(value) => unsafe { output(&value, buffer, capacity, required) },
        Err(()) => FAILED,
    })
    .unwrap_or(FAILED)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hepburn_handles_digraphs_nasals_long_vowels_and_split_verbs() {
        for (text, expected) in [
            ("シブヤ", "shibuya"),
            ("フラット", "furatto"),
            ("エンゼルタワー", "enzerutawaa"),
            ("シンヨウ", "shin'you"),
            ("マッチャ", "matcha"),
            ("ノッ", "noっ"),
        ] {
            assert_eq!(kana(text).unwrap(), expected);
        }
    }
}
