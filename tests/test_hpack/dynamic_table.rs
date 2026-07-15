use shiguredo_http2::hpack::DynamicTable;

#[test]
fn test_insert_and_get() {
    let mut table = DynamicTable::new(4096);

    table
        .insert(b"content-type", b"text/html")
        .expect("construction should succeed");
    assert_eq!(table.len(), 1);

    let entry = table.get(0).expect("value should be present");
    assert_eq!(entry.name(), b"content-type");
    assert_eq!(entry.value(), b"text/html");
}

#[test]
fn test_fifo_order() {
    let mut table = DynamicTable::new(4096);

    table
        .insert(b"first", b"1")
        .expect("construction should succeed");
    table.insert(b"second", b"2").expect("should succeed");
    table.insert(b"third", b"3").expect("should succeed");

    // 最新のエントリがインデックス 0
    assert_eq!(
        table.get(0).expect("value should be present").name(),
        b"third"
    );
    assert_eq!(
        table.get(1).expect("value should be present").name(),
        b"second"
    );
    assert_eq!(
        table.get(2).expect("value should be present").name(),
        b"first"
    );
}

#[test]
fn test_eviction() {
    // 小さいサイズ制限
    let mut table = DynamicTable::new(100);

    // エントリサイズ: 5 + 1 + 32 = 38
    table
        .insert(b"name1", b"1")
        .expect("construction should succeed");
    // エントリサイズ: 5 + 1 + 32 = 38
    table.insert(b"name2", b"2").expect("should succeed");
    assert_eq!(table.len(), 2);

    // 3つ目を追加すると最初のエントリが削除される
    table.insert(b"name3", b"3").expect("should succeed");
    assert_eq!(table.len(), 2);
    assert_eq!(
        table.get(0).expect("value should be present").name(),
        b"name3"
    );
    assert_eq!(
        table.get(1).expect("value should be present").name(),
        b"name2"
    );
}

#[test]
fn test_set_max_size() {
    let mut table = DynamicTable::new(4096);

    table
        .insert(b"name1", b"value1")
        .expect("construction should succeed");
    table.insert(b"name2", b"value2").expect("should succeed");
    assert_eq!(table.len(), 2);

    // RFC 7541 Section 4.2: 最大サイズ 0 の設定で動的テーブルのエントリを完全にクリアできる。
    table.set_max_size(0);
    assert_eq!(table.len(), 0);
}

#[test]
fn test_find() {
    let mut table = DynamicTable::new(4096);

    table
        .insert(b"content-type", b"text/html")
        .expect("construction should succeed");
    table
        .insert(b"content-type", b"application/json")
        .expect("should succeed");

    // 完全一致
    let result = table.find(b"content-type", b"application/json");
    assert_eq!(result, Some((0, true)));

    // 名前のみ一致
    let result = table.find(b"content-type", b"text/plain");
    assert_eq!(result, Some((0, false)));

    // 一致なし
    let result = table.find(b"x-custom", b"value");
    assert_eq!(result, None);
}

// RFC 7541 Section 4.4: 最大サイズより大きいエントリの追加はエラーではなく、テーブルを空にする。
#[test]
fn test_entry_too_large() {
    let mut table = DynamicTable::new(50);

    // このエントリは最大サイズより大きい
    table
        .insert(b"very-long-name", b"very-long-value")
        .expect("should succeed");

    // テーブルは空のまま
    assert!(table.is_empty());
}
