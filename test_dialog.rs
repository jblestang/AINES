use egui_file_dialog::FileDialog;
fn main() {
    let mut fd = FileDialog::new();
    fd.pick_file();
    let x = fd.take_picked();
}
