use crate::parser::Document;
use std::fs::{File, OpenOptions};
use std::io::Write;

pub struct OutputJson {
    path_prefix: String,
    file: File,
    counter: u16,
    page_limit: u16,
    file_counter: u16,
}

impl OutputJson {
    pub fn new(path_prefix: &str, page_limit: u16) -> Self {
        let file_path = format!("{}_0.json", &path_prefix);
        OutputJson {
            path_prefix: path_prefix.to_string(),
            file: OpenOptions::new()
                .write(true)
                .create(true)
                .open(file_path.as_str())
                .expect(
                    format!("can't open file[{}] with write option", file_path.as_str()).as_str(),
                ),
            counter: 0,
            page_limit,
            file_counter: 0,
        }
    }

    pub fn output(&mut self, doc: &mut Document) {
        let str = doc.to_json_string();
        writeln!(self.file, "{}", str);
        self.counter += 1;
        if self.counter == self.page_limit {
            self.file.flush();
            self.file_counter += 1;
            let path = format!("{}_{}.json", self.path_prefix, self.file_counter);
            self.file = OpenOptions::new()
                .write(true)
                .create(true)
                .open(path.as_str())
                .expect(format!("can't open file[{}] with write option", path.as_str()).as_str());
            self.counter = 0;
        }
    }

    pub fn flush(&mut self) {
        self.file.flush();
    }
}
