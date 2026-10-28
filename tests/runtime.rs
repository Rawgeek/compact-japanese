use compact_japanese::{Error, Romanizer};

fn model() -> Option<Romanizer> {
    let path = std::env::var_os("COMPACT_JAPANESE_DICTIONARY")?;
    Some(Romanizer::open(path).expect("test dictionary must open"))
}

#[test]
fn dictionary_contract_and_reference_spellings() {
    let Some(model) = model() else { return };
    for (input, expected) in [
        ("山田太郎", "Yamada Tarou"),
        ("佐々木", "Sasaki"),
        ("東京", "Tokyo"),
        ("大阪", "Osaka"),
        ("ホッピー", "Hoppy"),
        ("Coo Doo’s", "Coo Doo's"),
        ("", ""),
    ] {
        assert_eq!(model.romanize(input).unwrap(), expected);
    }
    assert_eq!(model.romanize("a\0b"), Err(Error::InvalidInput));
    assert_eq!(model.romanize(&"a".repeat(16385)), Err(Error::InvalidInput));
    assert_eq!(model.romanize("東京").unwrap(), "Tokyo");
}

#[test]
fn shared_mapping_is_safe_across_threads() {
    let Some(model) = model() else { return };
    let jobs: Vec<_> = (0..6)
        .map(|_| {
            let model = model.clone();
            std::thread::spawn(move || {
                for _ in 0..100 {
                    assert_eq!(
                        model.romanize("東京都渋谷区").unwrap(),
                        "Tokyo to Shibuya ku"
                    );
                }
            })
        })
        .collect();
    for job in jobs {
        job.join().unwrap();
    }
}

#[test]
fn missing_and_short_dictionary_fail_without_mapping() {
    let path =
        std::env::temp_dir().join(format!("compact-japanese-invalid-{}", std::process::id()));
    std::fs::write(&path, b"invalid").unwrap();
    assert!(matches!(Romanizer::open(&path), Err(Error::Dictionary)));
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(Romanizer::open(&path), Err(Error::Dictionary)));
}
