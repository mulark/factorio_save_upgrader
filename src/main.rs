extern crate nix;
extern crate directories;
extern crate glob;
#[macro_use]
extern crate lazy_static;

use std::sync::mpsc::Sender;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::sync::atomic::AtomicUsize;
use std::sync::Mutex;
use std::process::exit;
use std::fs::remove_file;
use std::fs::remove_dir_all;
use std::fs::read_to_string;
use std::io::Write;
use nix::unistd::Pid;
use std::path::PathBuf;
use nix::sys::signal::{kill, Signal};
use std::process::Command;
use directories::BaseDirs;
use glob::glob;
use std::env;
use std::time::{Duration, Instant};
use std::process::Stdio;
use nix::sys::wait::WaitStatus;
use std::fs::File;

const CAP_INSTANCES: usize = 2;

lazy_static! {
    static ref CURRENT_RESAVE_PORT: Mutex<u32> = Mutex::new(31498);
    static ref FINISHED: AtomicUsize = AtomicUsize::new(0);
}


fn get_factorio_path() -> [PathBuf; 4] {
        let base_dir = BaseDirs::new().unwrap();
        [
            std::env::current_dir().unwrap().join(""),
            std::env::current_dir().unwrap().join("bin").join("x64"),
            PathBuf::from("C:")
                .join("Program Files (x86)")
                .join("Steam")
                .join("steamapps")
                .join("common")
                .join("Factorio")
                .join(""),
            base_dir
                .home_dir()
                .join(".local")
                .join("share")
                .join("Steam")
                .join("steamapps")
                .join("common")
                .join("Factorio")
                .join(""),
        ]
}

pub fn get_executable_path() -> Option<PathBuf> {
    let exename = if cfg!(target_os = "linux") {
        "factorio"
    } else {
        "factorio.exe"
    };
    for p in &get_factorio_path() {
        if p.join(exename).exists() {
            return Some(p.join(exename));
        }
    }
    None
}

pub fn resave_dir() -> PathBuf {
    std::env::current_dir().unwrap().join("resave-configs")
}

/*
fn auto_resave_old(file_to_resave: PathBuf) -> Result<(),std::io::Error> {
    if !resave_dir().exists() {
        std::fs::create_dir(resave_dir())?;
    }

    let local_config_file_path = resave_dir().join(format!("{}{}", file_to_resave.file_name().unwrap().to_str().unwrap(), ".ini"));
    let mut local_config_file = File::create(&local_config_file_path).unwrap();
    println!("local config = {:?}", local_config_file_path);
    let local_write_dir = resave_dir().join(file_to_resave.file_name().unwrap()).join("");
    let local_logfile = local_write_dir.join("factorio-current.log");
    println!("running local files for {:?}", file_to_resave);
    writeln!(local_config_file, "[path]")?;
    writeln!(local_config_file, "read-data=__PATH__system-read-data__")?;
    writeln!(local_config_file, "write-data={}", local_write_dir.to_str().unwrap())?;
    writeln!(local_config_file, "[other]")?;
    //writeln!(local_config_file, "autosave-compression-level=maximum")?;
    let filesize = (file_to_resave.metadata().unwrap().len() / 1024) as f64 / 1024.0;
    println!("{:?}MB", filesize);
    let child = Command::new(get_executable_path().unwrap())
        .arg("--config")
        .arg(&local_config_file_path)
        .arg("--load-game")
        .arg(&file_to_resave)
        .arg("--mod-directory")
        .arg("test")
        .stdout(Stdio::null())
        .spawn()?;
    let pid = Pid::from_raw(child.id() as i32);
    std::thread::sleep(Duration::from_millis(1500));
    if let Ok(()) = kill(pid, Signal::SIGINT) {
        let expire = Instant::now() + Duration::from_millis(25000);
        let mut clean = false;
        let mut do_timeout = true;
        let mut last_line_content: String = "".to_string();
        while !last_line_content.contains("Goodbye") {
            let local_logfile = local_logfile.clone();
            if local_logfile.exists() {
                let read_buf = read_to_string(local_logfile).unwrap();
                let logfile_contents: Vec<_> = read_buf.lines().collect();
                last_line_content = logfile_contents[logfile_contents.len() - 1].to_string();
            }
            if last_line_content.contains("Saving progress:") {
                do_timeout = false;
            }
            if do_timeout && Instant::now() > expire {
                //if we have passed the timeout threshold and did not see the game being saved, exit uncleanly.
                break;
            }
            if last_line_content.contains("Goodbye") {
                clean = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if clean {
            remove_dir_all(local_write_dir)?;
            remove_file(local_config_file_path)?;
            //add hash to known_hashes
            println!("Child cleanly exited");
//            write_known_map_hash(sha256sum(file_to_resave));
        } else {
            eprintln!("Child did not cleanly exit for {:?}", file_to_resave);
            if let Ok(WaitStatus::StillAlive) = nix::sys::wait::waitpid(pid, None) {
                kill(pid, Signal::SIGKILL).expect("");
            } else {
                eprintln!("Wasnt stillalive");
            }
        }
    }
    Ok(())
}*/

fn main() {
    let args = env::args();
    if args.len() < 2 {
        eprintln!("Usage: save_upgrader *pattern*");
        std::process::exit(1);
    }
    let pattern = args.collect::<Vec<_>>()[1..].join(" ");
    let base = BaseDirs::new().unwrap();
    let save_dir = base.home_dir().join(".factorio").join("saves").join("");

    let glob_string = format!("{}{}*", save_dir.to_string_lossy(), pattern);
    println!("globby{:?}", glob_string);

    let mut handles = Vec::new();
    if let Ok(dir_listing) = glob(&glob_string) {
        let paths = dir_listing.filter_map(|x| x.ok()).filter(|y| y.extension().is_some()).collect::<Vec<_>>();
        let ct = paths.len();
        let (sendertx, receivertx) = channel();
        std::thread::spawn(move || {
            // Notifier thread
            let mut bucket: Vec<Sender<()>> = vec![];
            for _ in 0..ct {
                bucket.push(receivertx.recv().unwrap());
            }
            let mut expect = 0;
            loop {
                if bucket.is_empty() {
                    break;
                }
                for _ in 0..CAP_INSTANCES {
                    if let Some(tx) = bucket.pop() {
                        tx.send(()).unwrap();
                        expect += 1;
                    }
                }
                while FINISHED.load(Ordering::SeqCst) != expect {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        for f in paths {
            let (tx, rx) = channel::<()>();
            let sendertx = sendertx.clone();
            let h = std::thread::spawn(move|| {
                sendertx.send(tx).unwrap();
                rx.recv().unwrap();
                println!("thread spawned");
                println!("{:?}", f);
                println!("f: {:?}, ext {:?}", f, f.extension().unwrap());
                let res =  auto_resave(f);
                FINISHED.fetch_add(1, Ordering::SeqCst);
                match res {
                    Ok(clean) => println!("Process finished cleanly {}", clean),
                    Err(e) => eprintln!("Process encountered error {}", e),
                }
            });
            handles.push(h);
        }
    }
    for h in handles {
        h.join().unwrap();
    }
}

/// Returns Ok(true) if cleanly exited
pub fn auto_resave(file_to_resave: PathBuf) -> Result<bool, std::io::Error> {
    println!("resaving {:?}", file_to_resave);
    if !cfg!(target_os = "linux") {
        panic!("auto_resave is not supported on Windows!");
    }
    std::fs::create_dir_all(resave_dir())?;

    let local_config_file_path = resave_dir().join(format!(
        "{}{}",
        file_to_resave.file_name().unwrap().to_str().unwrap(),
        ".ini"
    ));
    let mut local_config_file = File::create(&local_config_file_path).expect("Some failure here!");
    let local_write_dir = resave_dir()
        .join(file_to_resave.file_name().unwrap())
        .join("");
    let local_mods_dir = local_write_dir.join("mods");
    let local_logfile = local_write_dir.join("factorio-current.log");
    // read-data=__PATH__executable__/../../data
    // write-data=__PATH__executable__/../..
    writeln!(local_config_file, "[path]")?;
    writeln!(local_config_file, "read-data=__PATH__executable__/../../data")?;
    writeln!(
        local_config_file,
        "write-data={}",
        local_write_dir.to_str().unwrap()
    )?;
    writeln!(local_config_file, "[other]")?;
    //writeln!(local_config_file, "autosave-compression-level=maximum")?;
    let port: u32;
    {
        let mut data = CURRENT_RESAVE_PORT.lock().unwrap();
        *data += 1;
        port = *data;
    }
    writeln!(local_config_file, "port={}", port)?;
    let child = Command::new(get_executable_path().unwrap())
        .arg("--config")
        .arg(&local_config_file_path)
        .arg("--start-server")
        .arg(&file_to_resave.canonicalize()?)
        .arg("--mod-directory")
        .arg(local_mods_dir)
        .stdout(Stdio::null())
        .spawn()?;
    let pid = Pid::from_raw(child.id() as i32);
    let mut clean = false;
    std::thread::sleep(Duration::from_millis(500));
    let mut file_text;
    let expire = Instant::now() + Duration::from_millis(30000);
    loop {
        //keep reading logfile until it's safe to send a SIGINT, or we fail, or we timeout.
        if Instant::now() > expire {
            //Incase the logfile never exists or never contains the lines we're looking for
            eprintln!("Timed out during busy loop waiting for log file to become ready.");
            eprintln!("{}", read_to_string(&local_logfile).unwrap());
            exit(1);
        }
        if local_logfile.exists() {
            file_text = read_to_string(&local_logfile).unwrap();
            if file_text.contains("Loading script.dat") {
                break;
            }
            if file_text.contains("Error") || file_text.contains("Failed") {
                eprintln!("An error was detected trying to resave maps.");
                eprintln!("Here is the factorio output for this moment.");
                eprintln!("{}", file_text);
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    if let Ok(()) = kill(pid, Signal::SIGINT) {
        let mut do_timeout = true;
        let mut last_line_content: String = "".to_string();
        while !last_line_content.contains("Goodbye") {
            let local_logfile = local_logfile.clone();
            if local_logfile.exists() {
                let read_buf = read_to_string(local_logfile).unwrap();
                let logfile_contents: Vec<_> = read_buf.lines().collect();
                last_line_content = logfile_contents[logfile_contents.len() - 1].to_string();
            }
            if last_line_content.contains("Saving progress:") {
                do_timeout = false;
            }
            if do_timeout && Instant::now() > expire {
                //if we have passed the timeout threshold and did not see the game being saved, exit uncleanly.
                eprintln!("A resave thread timed out");
                break;
            }
            if last_line_content.contains("Goodbye") {
                clean = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if clean {
            remove_dir_all(local_write_dir)?;
            remove_file(local_config_file_path)?;
        } else {
            eprintln!("Child did not cleanly exit for {:?}", file_to_resave);
            if let Ok(WaitStatus::StillAlive) = nix::sys::wait::waitpid(pid, None) {
                let res = kill(pid, Signal::SIGKILL);
                println!("{:?}", res);
            } else {
                panic!("Wasnt stillalive?");
            }
        }
    }
    Ok(clean)
}
