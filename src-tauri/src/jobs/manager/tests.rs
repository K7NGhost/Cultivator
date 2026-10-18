use super::*;
use std::sync::atomic::AtomicUsize;

fn register(manager: &JobManager) -> Arc<RunControl> {
    manager
        .register(
            "run",
            Arc::new(AtomicBool::new(false)),
            "case.sqlite".into(),
            "source".into(),
        )
        .unwrap()
}

#[tokio::test]
async fn individual_job_controls_do_not_cancel_or_pause_their_peers() {
    let manager = JobManager::with_limits(1, 1, 1);
    let run = register(&manager);
    let first = manager.register_job("run", "first", &run);
    let second = manager.register_job("run", "second", &run);
    manager.pause_job("first", true);
    let cancelled = first.cancelled.clone();
    let first_result = manager.enqueue_job(10, Workload::Cpu, &run, &first, async move {
        assert!(cancelled.load(Ordering::Acquire));
        Ok(())
    });
    let second_result = manager.enqueue_job(0, Workload::Cpu, &run, &second, async { Ok(()) });
    tokio::time::timeout(std::time::Duration::from_secs(5), second_result)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!run.cancelled.load(Ordering::Acquire));
    assert!(!second.cancelled.load(Ordering::Acquire));
    manager.cancel_job("first");
    tokio::time::timeout(std::time::Duration::from_secs(5), first_result)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!second.cancelled.load(Ordering::Acquire));
    manager.finish("run");
    manager.shutdown().await;
}

#[tokio::test]
async fn paused_queue_resumes_in_priority_then_fifo_order() {
    let manager = JobManager::with_limits(1, 1, 1);
    let control = register(&manager);
    manager.pause("run", true);
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    for (index, priority) in [(0, -10), (1, 10), (2, 10), (3, 0)] {
        let order = order.clone();
        handles.push(
            manager.enqueue(priority, Workload::Cpu, &control, async move {
                order.lock().unwrap().push(index);
                Ok(())
            }),
        );
    }
    assert!(order.lock().unwrap().is_empty());
    assert!(manager.pause("run", false));
    for handle in handles {
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    assert_eq!(*order.lock().unwrap(), vec![1, 2, 3, 0]);
    manager.finish("run");
    manager.shutdown().await;
}

#[tokio::test]
async fn cpu_and_disk_budgets_bound_simultaneous_jobs() {
    let manager = JobManager::with_limits(3, 2, 1);
    let control = register(&manager);
    let running = Arc::new(AtomicUsize::new(0));
    let max_running = Arc::new(AtomicUsize::new(0));
    let disk_running = Arc::new(AtomicUsize::new(0));
    let max_disk = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Semaphore::new(0));
    let (started, mut receiver) = tokio::sync::mpsc::channel(8);
    let mut handles = Vec::new();
    for index in 0..8 {
        let (running, max_running, disk_running, max_disk, gate, started) = (
            running.clone(),
            max_running.clone(),
            disk_running.clone(),
            max_disk.clone(),
            gate.clone(),
            started.clone(),
        );
        let disk = index < 4;
        handles.push(manager.enqueue(
            0,
            if disk { Workload::Disk } else { Workload::Cpu },
            &control,
            async move {
                let active = running.fetch_add(1, Ordering::SeqCst) + 1;
                max_running.fetch_max(active, Ordering::SeqCst);
                if disk {
                    let active = disk_running.fetch_add(1, Ordering::SeqCst) + 1;
                    max_disk.fetch_max(active, Ordering::SeqCst);
                }
                started.send(()).await.unwrap();
                let _permit = gate.acquire().await.unwrap();
                if disk {
                    disk_running.fetch_sub(1, Ordering::SeqCst);
                }
                running.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            },
        ));
    }
    for _ in 0..2 {
        tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(running.load(Ordering::SeqCst), 2);
    gate.add_permits(8);
    for handle in handles {
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    assert_eq!(max_running.load(Ordering::SeqCst), 2);
    assert_eq!(max_disk.load(Ordering::SeqCst), 1);
    manager.finish("run");
    manager.shutdown().await;
}

#[tokio::test]
async fn panicking_jobs_release_workers_and_cancelling_unpauses_pending_work() {
    let manager = JobManager::with_limits(1, 1, 1);
    let control = register(&manager);
    let failed = manager.enqueue(0, Workload::Cpu, &control, async {
        panic!("simulated plugin panic");
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), failed)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    manager.pause("run", true);
    let token = control.cancelled.clone();
    let cancelled = manager.enqueue(0, Workload::Cpu, &control, async move {
        assert!(token.load(Ordering::Acquire));
        Ok(())
    });
    assert!(manager.cancel("run"));
    tokio::time::timeout(std::time::Duration::from_secs(5), cancelled)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!manager.cancel("unknown"));
    manager.finish("run");
    manager.shutdown().await;
}

#[tokio::test]
async fn admission_is_bounded_and_datasource_locks_are_shared() {
    let manager = JobManager::with_limits(1, 1, 1);
    let permit = manager.reserve(256).unwrap();
    assert!(manager.reserve(1).is_err());
    assert!(manager.reserve(0).is_err());
    drop(permit);
    assert!(manager.reserve(1).is_ok());
    let first = manager.resource("case:source".into());
    let second = manager.resource("case:source".into());
    assert!(Arc::ptr_eq(&first, &second));
    let _guard = first.lock().await;
    assert!(second.try_lock().is_err());
    assert!(manager.resource("case:other".into()).try_lock().is_ok());
    manager.shutdown().await;
    assert!(manager.reserve(1).is_err());
}
