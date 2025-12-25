use reqwest::Client;
use std::io::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering}
};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::time::sleep;
use tokio::signal;

#[tokio::main]
async fn main() {
    // 保存上一次的输入
    let mut last_url = String::new();
    let mut last_method = String::new();
    let mut last_data = String::new();
    let mut last_threads = 1;
    let mut last_qps = 1; // 单线程 QPS

    loop {
        let url = prompt_default("请输入请求 URL", &last_url);
        let method = prompt_default("请输入请求方法 (GET/POST)", &last_method).to_uppercase();
        let data = if method == "POST" {
            prompt_default("请输入 POST 参数 (格式 key1=value1&key2=value2)", &last_data)
        } else {
            String::new()
        };
        let threads: usize = prompt_default("请输入线程数", &last_threads.to_string())
            .parse()
            .expect("线程数必须为整数");
        let qps: usize = prompt_default("请输入单线程每秒 QPS", &last_qps.to_string())
            .parse()
            .expect("单线程 QPS 必须为整数");

        // 保存本次输入
        last_url = url.clone();
        last_method = method.clone();
        last_data = data.clone();
        last_threads = threads;
        last_qps = qps;

        // 统计成功/失败
        let success_count = Arc::new(AtomicUsize::new(0));
        let failure_count = Arc::new(AtomicUsize::new(0));

        // 全局期望 QPS = 单线程 QPS * 线程数
        let expected_global_qps = qps * threads;

        // Semaphore 控制全局 QPS
        let semaphore = Arc::new(Semaphore::new(expected_global_qps));
        let client = Arc::new(Client::new());

        println!("开始压力测试，按 Ctrl+C 停止...");

        let start_time = Instant::now();
        let mut handles = vec![];

        for _ in 0..threads {
            let client = Arc::clone(&client);
            let semaphore = Arc::clone(&semaphore);
            let url = url.clone();
            let method = method.clone();
            let data = data.clone();
            let success_count = Arc::clone(&success_count);
            let failure_count = Arc::clone(&failure_count);

            let handle = tokio::spawn(async move {
                loop {
                    let permit = semaphore.acquire().await.unwrap();
                    let start = Instant::now();

                    let res = match method.as_str() {
                        "GET" => client.get(&url).send().await,
                        "POST" => client.post(&url).body(data.clone()).send().await,
                        _ => {
                            eprintln!("不支持的请求方法: {}", method);
                            return;
                        }
                    };

                    match res {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                success_count.fetch_add(1, Ordering::Relaxed);
                            } else {
                                failure_count.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Err(_) => {
                            failure_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    drop(permit);

                    let elapsed = start.elapsed();
                    let delay = Duration::from_secs_f64(1.0 / qps as f64);
                    if elapsed < delay {
                        sleep(delay - elapsed).await;
                    }
                }
            });

            handles.push(handle);
        }

        // 等待 Ctrl+C 信号
        signal::ctrl_c().await.unwrap();

        // 停止后统计信息
        let duration = start_time.elapsed().as_secs_f64();
        let succ = success_count.load(Ordering::Relaxed);
        let fail = failure_count.load(Ordering::Relaxed);
        let total = succ + fail;
        let actual_global_qps = if duration > 0.0 { total as f64 / duration } else { 0.0 };

        println!("\n--- 压力测试统计 ---");
        println!("请求地址: {}", url);
        println!("请求方式: {}", method);
        println!("成功数: {}", succ);
        println!("失败数: {}", fail);
        println!("期望全局 QPS: {}", expected_global_qps);
        println!("实际全局 QPS: {:.2}", actual_global_qps);

        println!("\n按回车开始新一轮压力测试，或 Ctrl+C 退出...");
        let mut dummy = String::new();
        io::stdin().read_line(&mut dummy).unwrap();
    }
}

// 支持默认值的提示函数
fn prompt_default(message: &str, default: &str) -> String {
    if default.is_empty() {
        print!("{}: ", message);
    } else {
        print!("{} [{}]: ", message, default);
    }
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim();
    if input.is_empty() {
        default.to_string()
    } else {
        input.to_string()
    }
}
