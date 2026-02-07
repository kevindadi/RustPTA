//! 跨函数指针分析测试用例
//! 测试指针在函数间传递、返回、间接调用

fn take_ref(x: &i32) -> &i32 {
    x
}

fn take_ref_mut(x: &mut i32) -> &mut i32 {
    x
}

fn pass_through() {
    let a = 1;
    let b = take_ref(&a);
    let _ = b;
}

fn return_ptr() -> Box<i32> {
    let x = 42;
    Box::new(x)
}

fn call_return_ptr() {
    let p = return_ptr();
    let _ = *p;
}

fn indirect_call(f: fn(&i32) -> &i32, x: &i32) -> &i32 {
    f(x)
}

fn test_indirect() {
    let val = 10;
    let r = indirect_call(take_ref, &val);
    let _ = *r;
}

fn main() {
    pass_through();
    call_return_ptr();
    test_indirect();
}
