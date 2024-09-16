use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use indicatif::ProgressBar;
use serde::Deserialize;
use ssh::{Session, RECURSIVE, WRITE};

#[derive(Deserialize)]
struct Config {
    host: String,
    path: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = format!("{}/.config/{}/config.yml", env::var("HOME")?, "rs-scp");

    if !Path::new(&config_path).exists() {
        std::fs::create_dir_all(Path::new(&config_path).parent().unwrap())?;
        let mut file = File::create(&config_path)?;
        file.write_all(b"host: \"\"\nuser: \"\"\npath: \"\"")?;
        println!("Config file created at: {}", config_path);
        return Ok(());
    }

    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage: <program_name> <file_to_upload>");
        return Ok(());
    }

    let config_file = File::open(config_path)?;
    let config: Config = serde_yaml::from_reader(config_file)?;

    // Establish an SSH session
    let mut session = Session::new().unwrap();
    session.set_host(&config.host).unwrap();
    session.parse_config(None).unwrap();
    session.connect().unwrap();
    session.userauth_publickey_auto(None).unwrap();

    let file_path = &args[1];
    let mut file = File::open(file_path)?;
    let file_size = file.metadata()?.len() as usize;

    let file_name = Path::new(file_path).file_name().unwrap().to_str().unwrap();

    let mut scp = session.scp_new(RECURSIVE | WRITE, config.path.as_str()).unwrap();
    scp.init().unwrap();
    scp.push_file(Path::new(file_name), file_size, 0o644).unwrap();

    const CHUNK_SIZE: usize = 1024;
    let mut buffer = vec![0; CHUNK_SIZE];

    let pb = ProgressBar::new(file_size as u64);
    pb.set_style(indicatif::ProgressStyle::default_bar()
        .template(" [{elapsed_precise}] [{wide_bar:40.cyan/blue}] {bytes}/{total_bytes} ({percent_precise}%) [{eta_precise}] ").unwrap()
        .progress_chars("=>-")
    );

    loop {
        let bytes_read = file.read(&mut buffer).unwrap();
        if bytes_read == 0 {
            break;
        }

        scp.write(&buffer[..bytes_read]).unwrap();
        pb.inc(bytes_read as u64);
    }

    pb.finish_and_clear();
    scp.flush().unwrap();
    scp.close();

    let url = match config.host.strip_suffix("/") {
        Some(host) => format!("https://{}/{}", host, file_name),
        None => format!("https://{}/{}", config.host, file_name),
    };
    println!("File uploaded successfully! URL: {}", url);

    Ok(())
}