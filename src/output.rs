use log::debug;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::time::Instant;

pub struct OutputManager {
    path_prefix: String,
    file_counter: i32,
}

impl OutputManager {
    pub fn new(path_prefix: &str) -> Self {
        OutputManager {
            path_prefix: path_prefix.to_string(),
            file_counter: 0,
        }
    }
    pub fn create_output_json(&mut self) -> OutputJson {
        let file_path = format!("{}_{}.json", &self.path_prefix, self.file_counter);
        self.file_counter += 1;
        return OutputJson::new(file_path.as_str());
    }
}

pub struct OutputJson {
    file: File,
    file_name: String,
    buffer: Vec<String>,
}

impl OutputJson {
    pub fn new(file_path: &str) -> Self {
        OutputJson {
            file: OpenOptions::new()
                .write(true)
                .create(true)
                .open(file_path)
                .expect(format!("can't open file[{}] with write option", file_path).as_str()),
            file_name: file_path.to_string(),
            buffer: vec![],
        }
    }

    pub fn output(&mut self, str: String) {
        self.buffer.push(str);
    }

    pub fn flush(&mut self) {
        debug!("Call {} flush...", self.file_name);
        for str in &self.buffer {
            writeln!(self.file, "{}", str);
        }
        // TODO need?
        self.file.flush();
        debug!("Finish {} flush...", self.file_name);
    }
}
