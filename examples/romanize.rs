use compact_japanese::Romanizer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: romanize <dictionary.bin> <normalized Japanese text>");
        std::process::exit(2);
    }
    println!("{}", Romanizer::open(&args[1])?.romanize(&args[2])?);
    Ok(())
}
