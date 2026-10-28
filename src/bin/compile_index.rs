use std::path::Path;
fn main() {
    let arguments: Vec<_> = std::env::args_os().collect();
    let result = if arguments.len() == 3 {
        compact_japanese::compile_index(Path::new(&arguments[1]), Path::new(&arguments[2]))
    } else {
        Err("expected source dictionary and destination".into())
    };
    match result {
        Ok((nodes, words, bytes)) => {
            println!("{{\"nodes\":{nodes},\"keys\":{words},\"size\":{bytes}}}")
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
