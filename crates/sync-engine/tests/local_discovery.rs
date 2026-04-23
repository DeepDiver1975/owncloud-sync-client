use camino::Utf8Path;
use std::fs;
use sync_engine::discovery::local::discover_local;
use tempfile::TempDir;

#[tokio::test]
async fn discovers_files_recursively() {
    let dir = TempDir::new().unwrap();
    let root = Utf8Path::from_path(dir.path()).unwrap();

    fs::write(dir.path().join("a.txt"), b"0123456789").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/b.txt"), b"hello").unwrap();
    fs::create_dir_all(dir.path().join("sub/deep")).unwrap();
    fs::write(dir.path().join("sub/deep/c.txt"), b"x").unwrap();

    let entries = discover_local(root).await.unwrap();

    assert!(entries.iter().all(|e| !e.is_virtual));

    let names: Vec<&str> = entries
        .iter()
        .map(|e| e.path.file_name().unwrap())
        .collect();

    assert!(names.contains(&"a.txt"), "missing a.txt");
    assert!(names.contains(&"b.txt"), "missing b.txt");
    assert!(names.contains(&"c.txt"), "missing c.txt");

    let a = entries
        .iter()
        .find(|e| e.path.file_name() == Some("a.txt"))
        .unwrap();
    assert_eq!(a.size, 10);
}

#[tokio::test]
async fn empty_directory_returns_empty_vec() {
    let dir = TempDir::new().unwrap();
    let root = Utf8Path::from_path(dir.path()).unwrap();
    let entries = discover_local(root).await.unwrap();
    assert!(entries.is_empty());
}

#[tokio::test]
async fn inodes_are_nonzero_on_linux() {
    let dir = TempDir::new().unwrap();
    let root = Utf8Path::from_path(dir.path()).unwrap();
    std::fs::write(dir.path().join("f.txt"), b"data").unwrap();
    let entries = discover_local(root).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].inode > 0);
}
