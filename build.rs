
fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/app.ico"); // 멀티 사이즈 포함 ico 권장
        res.compile().unwrap();
    }
}