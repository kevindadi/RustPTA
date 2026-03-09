#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$root/cases/deadlock" "$root/cases/datarace" "$root/cases/atomic"

for i in $(seq 1 10); do
  extra=$((i - 1))
  out="$root/cases/deadlock/dl_${i}.rs"
  {
    echo 'use std::sync::{Arc, Mutex};'
    echo 'use std::thread;'
    echo ''
    echo 'fn lock_ab(a: &Arc<Mutex<i32>>, b: &Arc<Mutex<i32>>) {'
    echo '    let _ga = a.lock().unwrap();'
    echo '    thread::yield_now();'
    echo '    let _gb = b.lock().unwrap();'
    echo '}'
    echo ''
    echo 'fn lock_ba(a: &Arc<Mutex<i32>>, b: &Arc<Mutex<i32>>) {'
    echo '    let _gb = b.lock().unwrap();'
    echo '    thread::yield_now();'
    echo '    let _ga = a.lock().unwrap();'
    echo '}'
    echo ''
    if [ "$extra" -gt 0 ]; then
      for j in $(seq 1 "$extra"); do
        prev=$((j - 1))
        if [ "$j" -eq 1 ]; then
          echo "fn helper_${j}(v: i32) -> i32 { v + 1 }"
        else
          echo "fn helper_${j}(v: i32) -> i32 { helper_${prev}(v) + 1 }"
        fi
      done
    fi
    echo ''
    echo 'fn main() {'
    echo '    let a = Arc::new(Mutex::new(0));'
    echo '    let b = Arc::new(Mutex::new(0));'
    echo '    let mut hs = Vec::new();'
    echo '    {'
    echo '        let a1 = Arc::clone(&a);'
    echo '        let b1 = Arc::clone(&b);'
    echo '        hs.push(thread::spawn(move || lock_ab(&a1, &b1)));'
    echo '    }'
    echo '    {'
    echo '        let a2 = Arc::clone(&a);'
    echo '        let b2 = Arc::clone(&b);'
    echo '        hs.push(thread::spawn(move || lock_ba(&a2, &b2)));'
    echo '    }'
    if [ "$extra" -gt 0 ]; then
      for j in $(seq 1 "$extra"); do
        echo "    hs.push(thread::spawn(move || { let _ = helper_${j}($j); }));"
      done
    fi
    echo '    for h in hs { let _ = h.join(); }'
    echo '}'
  } > "$out"
done

for i in $(seq 1 10); do
  extra=$i
  out="$root/cases/datarace/dr_${i}.rs"
  {
    echo 'use std::thread;'
    echo ''
    echo 'static mut COUNTER: i32 = 0;'
    echo ''
    echo 'unsafe fn bump_n(n: i32) {'
    echo '    for _ in 0..n {'
    echo '        COUNTER += 1;'
    echo '    }'
    echo '}'
    echo ''
    for j in $(seq 1 "$extra"); do
      if [ "$j" -eq 1 ]; then
        echo "unsafe fn path_${j}() { bump_n(10); }"
      else
        prev=$((j - 1))
        echo "unsafe fn path_${j}() { path_${prev}(); bump_n(10); }"
      fi
    done
    echo ''
    echo 'fn main() {'
    echo '    let mut hs = Vec::new();'
    echo "    for _ in 0..$((extra + 1)) {"
    echo '        hs.push(thread::spawn(|| unsafe {'
    echo "            path_${extra}();"
    echo '        }));'
    echo '    }'
    echo '    for h in hs { let _ = h.join(); }'
    echo '    unsafe { let _ = COUNTER; }'
    echo '}'
  } > "$out"
done

for i in $(seq 1 10); do
  extra=$i
  out="$root/cases/atomic/at_${i}.rs"
  {
    echo 'use std::sync::atomic::{AtomicUsize, Ordering};'
    echo 'use std::sync::Arc;'
    echo 'use std::thread;'
    echo ''
    echo 'fn load_then_work(v: &Arc<AtomicUsize>) -> usize {'
    echo '    let base = v.load(Ordering::Relaxed);'
    echo '    thread::yield_now();'
    echo '    base'
    echo '}'
    echo ''
    for j in $(seq 1 "$extra"); do
      if [ "$j" -eq 1 ]; then
        echo "fn writer_${j}(v: &Arc<AtomicUsize>) { v.store($j, Ordering::Relaxed); }"
      else
        prev=$((j - 1))
        echo "fn writer_${j}(v: &Arc<AtomicUsize>) { writer_${prev}(v); v.store($j, Ordering::Relaxed); }"
      fi
    done
    echo ''
    echo 'fn main() {'
    echo '    let v = Arc::new(AtomicUsize::new(0));'
    echo '    let mut hs = Vec::new();'
    echo '    {'
    echo '        let r = Arc::clone(&v);'
    echo '        hs.push(thread::spawn(move || { let _ = load_then_work(&r); }));'
    echo '    }'
    echo "    for _ in 0..$((extra + 1)) {"
    echo '        let w = Arc::clone(&v);'
    echo "        hs.push(thread::spawn(move || writer_${extra}(&w)));"
    echo '    }'
    echo '    for h in hs { let _ = h.join(); }'
    echo '}'
  } > "$out"
done

echo "generated cases under $root/cases"
