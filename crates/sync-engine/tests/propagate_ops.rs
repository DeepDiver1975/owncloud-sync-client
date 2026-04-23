use sync_engine::propagate::ops::{delete_remote, mkdir_remote, rename_remote};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn delete_remote_sends_delete() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/dav/spaces/space1/old.txt"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let url = Url::parse(&format!("{}/dav/spaces/space1/old.txt", server.uri())).unwrap();
    delete_remote(&url).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn mkdir_remote_sends_mkcol() {
    let server = MockServer::start().await;

    Mock::given(method("MKCOL"))
        .and(path("/dav/spaces/space1/newdir/"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let url = Url::parse(&format!("{}/dav/spaces/space1/newdir/", server.uri())).unwrap();
    mkdir_remote(&url).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn rename_remote_sends_move() {
    let server = MockServer::start().await;

    Mock::given(method("MOVE"))
        .and(path("/dav/spaces/space1/a.txt"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let from = Url::parse(&format!("{}/dav/spaces/space1/a.txt", server.uri())).unwrap();
    let to = Url::parse(&format!("{}/dav/spaces/space1/b.txt", server.uri())).unwrap();
    rename_remote(&from, &to).await.unwrap();
    server.verify().await;
}

#[tokio::test]
async fn delete_local_removes_file() {
    use camino::Utf8Path;
    use std::io::Write;
    use sync_engine::propagate::ops::delete_local;
    use tempfile::NamedTempFile;

    let mut f = NamedTempFile::new().unwrap();
    f.write_all(b"data").unwrap();
    f.flush().unwrap();
    let path = Utf8Path::from_path(f.path()).unwrap().to_owned();

    delete_local(&path).await.unwrap();
    assert!(!path.exists());
}
