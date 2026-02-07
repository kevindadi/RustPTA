//! 数组指针分析测试用例
//! 测试数组索引、slice、Index trait

fn array_index() {
    let arr = [1i32, 2, 3, 4, 5];
    let i = 2usize;
    let p = &arr[i];
    let _ = *p;
}

fn slice_index() {
    let arr = [1i32, 2, 3];
    let s: &[i32] = &arr;
    let p = &s[1];
    let _ = *p;
}

fn vec_index() {
    let v = vec![1i32, 2, 3];
    let p = &v[0];
    let _ = *p;
}

fn slice_range() {
    let arr = [1i32, 2, 3, 4, 5];
    let s: &[i32] = &arr;
    let sub = &s[1..3];
    let _ = sub;
}

fn main() {
    array_index();
    slice_index();
    vec_index();
    slice_range();
}
