use copypasta::{ClipboardContext, ClipboardProvider};
use indicatif::{MultiProgress, ProgressBar};
use is_terminal::IsTerminal;
use serde::{Deserialize, Serialize};
use ssh::{Session, RECURSIVE, WRITE};
use std::env;
use std::env::args;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use byte_unit::Byte;
use byte_unit::UnitType::Binary;

const FISH_COMPLETION_SCRIPT: &str = include_str!("fish.compl");

#[derive(Deserialize)]
struct Config {
    host: String,
    path: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct RemoteFile {
    name: String,
    size: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();

    if args().len() == 2 && args().nth(1).unwrap() == "--help" {
        println!("Usage: rs-scp <files_to_upload/file_name (only if file is piped)>");
        println!("Arguments:");
        println!("\t--help: Display this help message.");
        println!("\t--list: List all the files on the server.");
        println!("\t--remove <file_name>: Remove a file from the server.");
        println!("\t--json: Display output as JSON. Currently only works with --list.");
        println!("\t--fish: Returns the fish completion code. You can add this \"rs-scp --fish | source\" to your ~/.config/fish/config.fish");
        println!("\nYou can also pipe the output of rs-scp to get the URL.");
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

    let args: Vec<String> = args().collect();
    if args.len() < 2 {
        println!("use rs-scp --help for more information.");
        return Ok(());
    }

    let mut json: bool = false;
    for arg in &args[1..] {
        if arg == "--json" {
            json = true;
        }
        if arg == "--fish" {
            println!("{}", FISH_COMPLETION_SCRIPT);
            return Ok(());
        }
    }

    let config_file = File::open(config_path)?;
    let config: Config = serde_yaml::from_reader(config_file)?;

    // Establish an SSH session
    let mut session = Session::new().unwrap();
    session.set_host(&config.host).unwrap();
    session.parse_config(None).unwrap();
    session.connect().unwrap();
    session.userauth_publickey_auto(None).unwrap();

    let multi = MultiProgress::new();

    for arg in &args {
        if arg == &args[0] || arg == "--json" {
            continue;
        }

        let file_name: &str;
        let arg1 = &arg.to_owned();

        if arg1 == "--list" {
            let path = config.path.as_str();

            let mut channel = session.channel_new().unwrap();
            channel.open_session().unwrap();
            channel.request_exec(format!("ls -lAh {}", path).as_bytes()).unwrap();
            let mut buffer = [0; 1024];
            let mut data = String::new();
            while channel.stdout().read(&mut buffer).unwrap() > 0 {
                data.push_str(std::str::from_utf8(&buffer).unwrap());
            }
            channel.close();
            if json {
                let mut filenames = Vec::<RemoteFile>::new();
                for line in data.lines().skip(1) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() > 8 {
                        let file_name = parts[8..].join(" ");
                        let file_size = parts[4].to_owned();
                        filenames.push(RemoteFile { name: file_name, size: file_size });
                    }
                }
                println!("{}", serde_json::to_string(&filenames).unwrap());
            } else {
                println!("{}", data);
            }
            return Ok(());
        } else if arg1.starts_with("--remove") {
            let path = config.path.strip_suffix("/").unwrap();

            let file_name = args[2..].to_owned();
            let mut channel = session.channel_new().unwrap();
            channel.open_session().unwrap();
            channel.request_exec(format!("rm {}", file_name.iter().map(|x| format!("{}/{}", path, x.replace(" ", "\\ "))).collect::<Vec<String>>().join(" ")).as_bytes()).unwrap();
            let mut buffer = [0; 4096];
            let mut data = String::new();
            while channel.stderr().read(&mut buffer).unwrap() > 0 {
                data.push_str(std::str::from_utf8(&buffer).unwrap());
            }
            channel.close();
            if data.is_empty() {
                println!("File{} removed successfully!", if file_name.len() == 1 { "" } else { "s" });
            } else {
                println!("{}", data);
            }
            return Ok(());
        }

        const CHUNK_SIZE: usize = 1024;
        if atty::is(atty::Stream::Stdin) {
            let file_path = arg1;
            let mut file = File::open(file_path)?;
            let file_size = file.metadata()?.len() as usize;

            file_name = Path::new(file_path).file_name().unwrap().to_str().unwrap();

            let mut scp = session.scp_new(RECURSIVE | WRITE, config.path.as_str()).unwrap();
            scp.init().unwrap();
            scp.push_file(Path::new(file_name), file_size, 0o644).unwrap();

            let mut buffer = vec![0; CHUNK_SIZE];

            let progress_line = "{binary_bytes_per_sec} {elapsed_precise} [{bar:40}] {percent_precise}%";

            let pb = ProgressBar::new(file_size as u64);
            pb.set_style(indicatif::ProgressStyle::default_bar()
                .template(&format!("{{wide_msg}} {{bytes}} {progress_line}")).unwrap()
                .progress_chars("#--")
            );
            pb.set_message(file_name.to_string());

            loop {
                let bytes_read = file.read(&mut buffer).unwrap();
                if bytes_read == 0 {
                    break;
                }

                scp.write(&buffer[..bytes_read]).unwrap();
                pb.inc(bytes_read as u64);
            }

            let index = args.iter().position(|x| x == arg1).unwrap();
            let human_file_size = Byte::from_u64(file_size as u64).get_appropriate_unit(Binary);
            let prefix = if args.len() > 2 { format!("[{index}] ") } else { "".to_string() };
            pb.set_style(indicatif::ProgressStyle::default_bar()
                .template(&format!("{prefix}{{wide_msg}} {human_file_size:.2} {{elapsed_precise}} [{{bar:10}}] {{percent_precise}}%")).unwrap()
                .progress_chars("#--")
            );
            pb.set_position(file_size as u64);
            pb.finish();
            multi.add(pb);
            scp.flush().unwrap();
            scp.close();
        } else {
            file_name = arg1;

            let mut buffer = Vec::new();
            stdin.read_to_end(&mut buffer).unwrap();
            let file_size = buffer.len();

            let mut scp = session.scp_new(RECURSIVE | WRITE, config.path.as_str()).unwrap();
            scp.init().unwrap();
            scp.push_file(Path::new(file_name), file_size, 0o644).unwrap();

            scp.write(&buffer).unwrap();

            scp.flush().unwrap();
            scp.close();
        }

        let ctx = ClipboardContext::new();

        let url = match config.host.strip_suffix("/") {
            Some(host) => format!("https://{}/{}", host, file_name),
            None => format!("https://{}/{}", config.host, file_name),
        };

        if args.len() < 3 {
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

                if use_clipboard && !ctx.is_err() {
                    ctx.unwrap().set_contents(url.to_owned()).unwrap();
                }

                println!("URL copied to clipboard!");
            } else {
                print!("{}", url);
            }
        }
    }

    Ok(())
}