use failure::Error;
use notify::{watcher, DebouncedEvent, RecommendedWatcher, RecursiveMode, Watcher};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

#[derive(Debug)]
enum Event {
    Create(PathBuf),
    Remove(PathBuf),
    Rename(PathBuf, PathBuf),
    Error(Error, Option<PathBuf>),
}

#[derive(Debug)]
struct EventType {
    index: usize,
    event: Event,
}

#[derive(Debug, Clone, Eq)]
struct Input {
    index: usize,
    path: PathBuf,
    priority: u32,
}

impl Event {
    fn from(input: &Input, event: DebouncedEvent) -> Result<Option<EventType>, Error> {
        let event: Option<Event> = match event {
            DebouncedEvent::Create(path) => {
                Some(Event::Create(path.strip_prefix(&input.path)?.to_path_buf()))
            }
            DebouncedEvent::Remove(path) => {
                Some(Event::Remove(path.strip_prefix(&input.path)?.to_path_buf()))
            }
            DebouncedEvent::Rename(from, to) => Some(Event::Rename(
                from.strip_prefix(&input.path)?.to_path_buf(),
                to.strip_prefix(&input.path)?.to_path_buf(),
            )),
            DebouncedEvent::Error(e, path) => Some(Event::Error(e.into(), path)),
            _ => None,
        };

        Ok(match event {
            Some(event) => Some(EventType {
                index: input.index,
                event,
            }),
            None => None,
        })
    }
}

impl Input {
    fn build_watcher(&self, transmiter: Sender<EventType>) -> Result<(), Error> {
        let input = self.clone();

        thread::spawn(move || {
            let (tx, rx): (Sender<DebouncedEvent>, Receiver<DebouncedEvent>) = mpsc::channel();

            let mut watcher: RecommendedWatcher = watcher(tx, Duration::from_secs(1)).unwrap();
            watcher
                .watch(&input.path, RecursiveMode::Recursive)
                .unwrap();

            loop {
                match rx.recv() {
                    Ok(event) => {
                        if let Some(event) = Event::from(&input, event).unwrap() {
                            transmiter.send(event).unwrap();
                        }
                    }
                    Err(e) => {
                        transmiter
                            .send(EventType {
                                index: input.index,
                                event: Event::Error(e.into(), None),
                            })
                            .unwrap();

                        println!("Unrecoverable error on watcher {}: {:?}", input.index, e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }
}

impl Ord for Input {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl PartialOrd for Input {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

#[derive(Debug, Clone, Default)]
pub struct Overlay {
    inputs: Vec<Input>,
    output: PathBuf,
    input_map: HashMap<PathBuf, BinaryHeap<Input>>,
}

impl Overlay {
    pub fn new(path: &str) -> Self {
        Overlay {
            inputs: vec![],
            output: Path::new(path).to_path_buf(),
            input_map: HashMap::new(),
        }
    }

    pub fn add_input(&mut self, path: &str, priority: u32) -> usize {
        self.inputs.push(Input {
            index: self.inputs.len(),
            path: Path::new(path).to_path_buf(),
            priority,
        });

        self.inputs.len() - 1
    }

    fn build_watchers(&self) -> Result<Receiver<EventType>, Error> {
        let (tx, rx): (Sender<EventType>, Receiver<EventType>) = mpsc::channel();

        // NOTE: There should be a watcher on Output.
        // NOTE: That moves files not created by Overlay to highest priority Input.

        for index in 0..self.inputs.len() {
            self.inputs[index].build_watcher(tx.clone())?;
        }

        Ok(rx)
    }

    fn process_event(&mut self, event: EventType) {
        print!("{:?}", &event);

        match event.event {
            Event::Create(path) => {
                let input = &self.inputs[event.index];
                if let Some(input_heap) = self.input_map.get_mut(&path) {
                    if let Some(highest_prio) = input_heap.peek() {
                        if input.priority <= highest_prio.priority {
                            println!(" IGNORED!");
                            input_heap.push(input.clone());
                            return;
                        }
                    }
                }

                let mut input_file = input.path.clone();
                let mut output_file = self.output.clone();

                input_file.push(&path);
                output_file.push(&path);

                if output_file.exists() {
                    print!(" DELETED,");
                    fs::remove_file(&output_file).unwrap();
                }
                if fs::hard_link(input_file, output_file).is_ok() {
                    print!(" LINKED!");
                    self.input_map
                        .entry(path)
                        .or_insert_with(BinaryHeap::new)
                        .push(input.clone());
                }
            }
            Event::Remove(path) => {
                let mut output_file = self.output.clone();
                output_file.push(&path);

                if let Some(input_heap) = self.input_map.get_mut(&path) {
                    if let Some(highest_prio) = input_heap.peek() {
                        if event.index == highest_prio.index {
                            input_heap.pop();
                            print!(" DELETED!");
                            fs::remove_file(&output_file).unwrap();
                            if let Some(highest_prio) = input_heap.peek() {
                                let mut input_file = highest_prio.path.clone();
                                input_file.push(&path);

                                if fs::hard_link(input_file, output_file).is_ok() {
                                    print!(" LINKED!");
                                }
                            }
                        }
                    }
                }
            }
            Event::Rename(_from, _to) => {
                //TODO: Implement this >..>
            }
            _ => {}
        }

        println!();
    }

    pub fn process_loop(&mut self) -> Result<(), Error> {
        // TODO: Clean output directory.
        // TODO: Walk input directories and build output directory.

        let rx: Receiver<EventType> = self.build_watchers()?;

        loop {
            self.process_event(rx.recv()?);
        }
    }
}
