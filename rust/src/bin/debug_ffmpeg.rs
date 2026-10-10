use std::process::Command;

fn main() {
    println!("Debugging FFmpeg detection...");

    // 1. Check env var
    if let Ok(ffmpeg_path) = std::env::var("FFMPEG_PATH") {
        println!("FFMPEG_PATH env var is set: {}", ffmpeg_path);
        check_path(&ffmpeg_path);
    } else {
        println!("FFMPEG_PATH env var is NOT set");
    }

    // 2. Check bundled
    if let Ok(exe_path) = std::env::current_exe() {
        println!("Current executable path: {:?}", exe_path);
        if let Some(exe_dir) = exe_path.parent() {
            println!("Executable directory: {:?}", exe_dir);
            // ... (check bundled logic from video.rs) ...
        }
    }

    // 3. Check common paths
    let paths = vec![
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
        "ffmpeg",
    ];

    for path in paths {
        println!("Checking path: {}", path);
        check_path(path);
    }
}

fn check_path(path: &str) {
    let exists = std::path::Path::new(path).exists();
    println!("  -> Exists (std::fs): {}", exists);

    let which_res = which::which(path);
    println!("  -> Which result: {:?}", which_res);

    if exists || which_res.is_ok() {
        println!("  -> Attempting to run {} -version...", path);
        match Command::new(path).arg("-version").output() {
            Ok(output) => {
                println!("  -> Execution successful. Status: {}", output.status);
            }
            Err(e) => {
                println!("  -> Execution failed: {}", e);
            }
        }
    }
}
