use std::env;
use std::env::args;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use copypasta::{ClipboardContext, ClipboardProvider};
use indicatif::ProgressBar;
use is_terminal::IsTerminal;
use serde::Deserialize;
use ssh::{Session, RECURSIVE, WRITE};

#[derive(Deserialize)]
struct Config {
    host: String,
    path: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if args().len() == 2 && args().nth(1).unwrap() == "--help" {
        println!("Usage: rs-scp <file_to_upload>");
        println!("You can also pipe the output of rs-scp to get the URL.");
        println!("\nVersion: {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let config_path = format!("{}/.config/{}/config.yml", env::var("HOME")?, "rs-scp");

    if !Path::new(&config_path).exists() {
        std::fs::create_dir_all(Path::new(&config_path).parent().unwrap())?;
        let mut file = File::create(&config_path)?;
        file.write_all(b"host: \"\"# Server you want to upload to.\npath: \"\"# The path on the server where your files should be saved.")?;
        println!("Config file created at: {}", config_path);
        return Ok(());
    }

    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage: rs-scp <file_to_upload>");
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

    let mut ctx = ClipboardContext::new().unwrap();

    let url = match config.host.strip_suffix("/") {
        Some(host) => format!("https://{}/{}", host, file_name),
        None => format!("https://{}/{}", config.host, file_name),
    };


    if std::io::stdout().is_terminal() {
        println!("File uploaded successfully! URL: {}", url);

        let mut use_clipboard = false;

        if env::var("WAYLAND_DISPLAY").is_ok() {
            let mut child = Command::new("wl-copy")
                .stdin(Stdio::piped())
                .spawn()
                .expect("Failed to copy URL to clipboard");

            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(url.as_bytes()).unwrap();

            let output = child.wait_with_output().unwrap();
            if !output.status.success() {
                use_clipboard = true;
            }
        } else if env::var("DISPLAY").is_ok() {
            use_clipboard = true;
        } else {
            use_clipboard = true;
        }

        if use_clipboard {
            ctx.set_contents(url.to_owned()).unwrap();
        }

        println!("URL copied to clipboard!");
    } else {
        print!("{}", url);
    }

    Ok(())
}