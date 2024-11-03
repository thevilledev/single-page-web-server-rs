use memoffset;

use single_page_web_server_rs::server::AppState;

#[test]
    fn check_memory_layout() {
        println!("AppState size: {} bytes", std::mem::size_of::<AppState>());
        println!("AppState alignment: {} bytes", std::mem::align_of::<AppState>());
        
        // Print individual field offsets
        println!("etag offset: {}", memoffset::offset_of!(AppState, etag));
        println!("compressed_content_length offset: {}", 
            memoffset::offset_of!(AppState, compressed_content_length));
    }