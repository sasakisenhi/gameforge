use gameforge_desktop::DesktopConfig;

#[test]
fn defaults_to_three_concurrent_task_runs() {
    let config = DesktopConfig::from_lookup(|_| None).unwrap();

    assert_eq!(config.max_concurrent_task_runs(), 3);
}

#[test]
fn reads_a_positive_concurrency_override() {
    let config = DesktopConfig::from_lookup(|key| {
        (key == "GAMEFORGE_MAX_CONCURRENT_TASK_RUNS").then(|| "5".to_owned())
    })
    .unwrap();

    assert_eq!(config.max_concurrent_task_runs(), 5);
}

#[test]
fn rejects_zero_concurrency() {
    let error = DesktopConfig::from_lookup(|_| Some("0".to_owned())).unwrap_err();

    assert!(error.to_string().contains("positive integer"));
}
