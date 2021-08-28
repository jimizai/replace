use clap::{App, Arg};
use core::panic;
use crossbeam_utils::thread;
use deque::{Stealer, Stolen};
use ignore::WalkBuilder;
use num_cpus;
use regex::Regex;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use std::sync::Arc;

enum Work {
    File(String),
    Quit,
}

struct Worker {
    chan: Stealer<Work>,
}

impl Worker {
    fn run<F>(self, replace_fn: F)
    where
        F: Fn(&str),
    {
        loop {
            match self.chan.steal() {
                Stolen::Empty | Stolen::Abort => continue,
                Stolen::Data(Work::Quit) => break,
                Stolen::Data(Work::File(path)) => {
                    replace_fn(&path);
                }
            }
        }
    }
}

struct FileControl<'a> {
    path: &'a str,
}

impl<'a> FileControl<'a> {
    #[inline]
    fn new(path: &'a str) -> Self {
        Self { path }
    }

    fn lines(&self) -> io::Result<io::Lines<io::BufReader<File>>> {
        let file = File::open(self.path)?;
        Ok(io::BufReader::new(file).lines())
    }

    fn replace(&self, regex: Regex, re: &str) -> io::Result<()> {
        let lines = self.lines()?;
        let mut bytes: Vec<u8> = vec![];
        for line in lines {
            let line = line?;
            let mut result = regex.replace_all(&line, re).to_string();
            result.push_str("\n");
            bytes.extend_from_slice(result.as_bytes());
        }

        fs::write(self.path, bytes)?;
        Ok(())
    }
}

#[derive(Debug)]
struct Replacements<'a> {
    origin: &'a str,
    replace: &'a str,
}

impl<'a> Replacements<'a> {
    pub fn new(replace: &'a str) -> Self {
        let mut v = replace.split(":");
        let origin = v.next().expect("Field replace error to parse");
        let replace = v.next().expect("Field replace error to parse");
        if origin.is_empty() {
            panic!("origin could not be empty");
        }
        Self { origin, replace }
    }

    pub fn replace_all(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let regex = Regex::new(self.origin)?;
        FileControl::new(path).replace(regex, self.replace)?;
        Ok(())
    }
}

fn run<'a>(dir: &Path, replace: &'a str) {
    thread::scope(move |s| {
        let replace_ment = Arc::new(Replacements::new(replace));
        let cpus = num_cpus::get();
        let (workq, stealer) = deque::new();
        let mut workers = vec![];
        for _ in 0..cpus {
            let worker = Worker {
                chan: stealer.clone(),
            };
            let replace_ment = replace_ment.clone();
            workers.push(s.spawn(move |_| {
                worker.run(|path: &str| {
                    match replace_ment.replace_all(path).map_err(|e| e.to_string()) {
                        Ok(_) => {
                            println!("replaced path {} succeed", path)
                        }
                        Err(err) => {
                            println!("failed to replace path {}, err is {}", path, err)
                        }
                    }
                })
            }))
        }

        let walker = WalkBuilder::new(dir).build();
        let files = walker
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().expect("no filetype").is_file())
            .map(|entry| String::from(entry.path().to_str().unwrap()));

        for path in files {
            workq.push(Work::File(path))
        }

        for _ in 0..workers.len() {
            workq.push(Work::Quit)
        }

        for worker in workers {
            worker.join().unwrap()
        }
    })
    .unwrap()
}

fn main() {
    let matches = App::new("mr")
        .bin_name("mr")
        .version("0.0.1")
        .arg(
            Arg::with_name("dir")
                .short("d")
                .long("dir")
                .value_name("dir")
                .help("Need dir")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("replace")
                .short("r")
                .value_name("replace")
                .long("replace")
                .help("Need replace origin")
                .takes_value(true),
        )
        .get_matches();
    let dir = matches.value_of("dir").expect("Field dir is required");
    let replace = matches
        .value_of("replace")
        .expect("Field replace is required");
    if !replace.contains(":") {
        println!("Field replace must includes :");
    }

    let path = Path::new(dir);
    run(path, replace);
}
