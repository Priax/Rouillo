fn main() {
    #[cfg(target_os = "windows")]
    winresource::WindowsResource::new().set_icon("../assets/puyo_puyo_icon.ico").compile().unwrap();
}
