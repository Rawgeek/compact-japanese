use std::path::Path;
fn main() {
    let arguments: Vec<_> = std::env::args_os().collect();
    let result = if arguments.len() == 4 {
        compact_japanese::compile_matrix(
            Path::new(&arguments[1]),
            Path::new(&arguments[2]),
            Path::new(&arguments[3]),
        )
    } else {
        Err("expected source matrix, destination and context maps".into())
    };
    match result {
        Ok((rows, columns, bytes)) => {
            println!("{{\"rows\":{rows},\"columns\":{columns},\"size\":{bytes}}}")
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
