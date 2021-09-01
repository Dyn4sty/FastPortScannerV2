extern crate clap;

use clap::{load_yaml, App};
use serde_json::{from_str, Map, Value};
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::ops::RangeInclusive;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};
use threadpool::ThreadPool;

const DEFAULT_PORT_RANGE: RangeInclusive<u16> = 0..=65535;

#[derive(Debug)]
struct Options {
    ports: RangeInclusive<u16>,
    target: String,
    threads_number: i32,
    connection_timeout_time: f32,
    thread_sleep_time: f32,
    output_file: Option<String>,
    open_ports: Vec<String>,
    common_ports: Map<String, Value>,
}

fn get_ip(hostname: &str) -> String {
    let socket = format!("{}:{}", hostname, 80)
        .to_socket_addrs()
        .expect("Unable to resolve domain")
        .next()
        .unwrap();
    return socket.ip().to_string();
}

fn get_options() -> Options {
    let yaml = load_yaml!("cli.yaml");
    let matches = App::from(yaml).get_matches();
    let config = match fs::read_to_string("common_ports.json") {
        Ok(v) => v,
        Err(_) => {
            println!("Error: Cant find common ports file");
            std::process::exit(-1);
        },
    };
    let parsed: Value = from_str(&config).unwrap();
    let common_ports: Map<String, Value> = parsed.as_object().unwrap().clone();
    let ports: Vec<u16> = matches
        .values_of("port")
        .unwrap()
        .map(|port| match port.parse::<u16>() {
            Ok(x) => x,
            Err(err) => panic!("Error: {}", err),
        })
        .collect();
    let ports: RangeInclusive<u16> = if ports.len() == 1 {
        if ports[0] == 0 {
            DEFAULT_PORT_RANGE
        } else {
            ports[0]..=ports[0]
        }
    } else {
        ports[0]..=ports[1]
    };
    let target = matches.value_of("target").unwrap().to_owned();
    let threads_number = matches
        .value_of("threads_number")
        .unwrap()
        .parse()
        .expect("valid integer");
    let output_file = matches.value_of("output_file").map(String::from);
    let connection_timeout_time: f32 = matches
        .value_of("connection_timeout_time")
        .unwrap()
        .parse::<f32>()
        .unwrap();
    let thread_sleep_time: f32 = matches
        .value_of("thread_sleep_time")
        .unwrap()
        .parse::<f32>()
        .unwrap();
    let open_ports = Vec::with_capacity((*ports.end() - *ports.start()) as usize);
    let options = Options {
        ports,
        target,
        threads_number,
        output_file,
        connection_timeout_time,
        thread_sleep_time,
        open_ports,
        common_ports,
    };

    options // same as return options;
}

fn port_checker(host: &str, port: u16, connection_timeout_time: f32, tx: mpsc::Sender<u16>) {
    if TcpStream::connect_timeout(
        &(host, port).to_socket_addrs().unwrap().next().unwrap(),
        Duration::from_secs_f32(connection_timeout_time),
    )
    .is_ok()
    {
        tx.send(port).unwrap();
    };
}

fn launch_thread(options: &mut Options, port: u16, tx: mpsc::Sender<u16>, pool: &ThreadPool) {
    let target = options.target.clone();
    let connection_timeout_time = options.connection_timeout_time;

    pool.execute(move || port_checker(&target, port, connection_timeout_time, tx));
}

fn start_scan_thread(options: &mut Options, tx: mpsc::Sender<u16>, pool: &ThreadPool) {
    println!(
        "Scanning ports by range:\nFROM PORT {} TO PORT {}\n-----------------",
        options.ports.start(),
        options.ports.end()
    );

    for port in options.ports.clone() {
        launch_thread(options, port, tx.clone(), &pool);
    }
}

fn write_to_output_file(filename: &str, data: &str) {
    fs::write(filename, data).expect("Unable to write file");
}

fn main() {
    let mut options = get_options();
    let hostname = options.target;
    options.target = get_ip(&hostname);
    let start_time = SystemTime::now();

    let (tx, rx): (mpsc::Sender<u16>, mpsc::Receiver<u16>) = mpsc::channel();
    let pool = ThreadPool::new(options.threads_number as usize);
    start_scan_thread(&mut options, tx, &pool);
    let receiver_thread = thread::spawn(move || {
        for port in rx {
            println!("[+] Open Port: {}", port);
            let service = match options.common_ports.get(&port.to_string()) {
                Some(v) => v.as_str().unwrap(),
                None => "",
            };
            options.open_ports.push(format!("{} {}", port, service));
        }
        options
    });

    pool.join();
    options = receiver_thread.join().unwrap();
    let total_time = SystemTime::now()
        .duration_since(start_time)
        .unwrap()
        .as_secs_f64();
    if options.output_file.is_some() {
        write_to_output_file(
            &options.output_file.unwrap(),
            &format!(
                "[{}] <{}> Open Ports: {:?}\n",
                "date", hostname, options.open_ports
            ),
        );
    }
    println!(
        "\nOpen Ports: {:?} \nPort scanning has been completed in {} second(s)!",
        options.open_ports, total_time
    )
}
