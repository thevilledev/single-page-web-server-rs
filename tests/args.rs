
use clap::Parser;
use single_page_web_server_rs::cli::Args;

#[test]
fn test_args() -> Result<(), Box<dyn std::error::Error>> {
    temp_env::with_vars_unset(&["WEB_PORT", "WEB_ADDR", "WEB_INDEX_PATH"], || {
        let args = Args::try_parse_from(&["program"]).unwrap();
        assert_eq!(args.port, 3000);
        assert_eq!(args.addr, "127.0.0.1");
        assert_eq!(args.index_path, "index.html");

        let args = Args::try_parse_from(&["program", "--port", "8080"]).unwrap();
        assert_eq!(args.port, 8080);
        assert_eq!(args.addr, "127.0.0.1");
        assert_eq!(args.index_path, "index.html");

        let args = Args::try_parse_from(&["program", "--addr", "0.0.0.0", "--port", "8080"]).unwrap();
        assert_eq!(args.port, 8080);
        assert_eq!(args.addr, "0.0.0.0");
        assert_eq!(args.index_path, "index.html");
    });

    Ok(())
}

#[test]
fn test_args_env() -> Result<(), Box<dyn std::error::Error>> {
    // Test environment variables
    temp_env::with_vars(vec![
        ("WEB_PORT", Some("9090")),
        ("WEB_ADDR", Some("0.0.0.0")),
        ("WEB_INDEX_PATH", Some("/tmp/foo.html"))
    ], || {
        let args = Args::try_parse_from(&["program"]).unwrap();
        assert_eq!(args.port, 9090);
        assert_eq!(args.addr, "0.0.0.0");
        assert_eq!(args.index_path, "/tmp/foo.html");
    });


    // Test CLI args override environment variables
    temp_env::with_vars(vec![
        ("WEB_PORT", Some("9090")),
        ("WEB_ADDR", Some("0.0.0.0")),
    ], || {
        let args = Args::try_parse_from(&["program", "--port", "8080"]).unwrap();
        assert_eq!(args.port, 8080); // CLI arg takes precedence
        assert_eq!(args.addr, "0.0.0.0"); // ENV var is used
    });

    Ok(())
}