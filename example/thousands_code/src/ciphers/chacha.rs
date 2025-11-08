macro_rules! quarter_round {
    ($a:expr,$b:expr,$c:expr,$d:expr) => {
        $a = $a.wrapping_add($b);
        $d = ($d ^ $a).rotate_left(16);
        $c = $c.wrapping_add($d);
        $b = ($b ^ $c).rotate_left(12);
        $a = $a.wrapping_add($b);
        $d = ($d ^ $a).rotate_left(8);
        $c = $c.wrapping_add($d);
        $b = ($b ^ $c).rotate_left(7);
    };
}

#[allow(dead_code)]

pub const C: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];





































































pub fn chacha20(input: &[u32; 16], output: &mut [u32; 16]) {
    output.copy_from_slice(&input[..]);
    for _ in 0..10 {
        
        quarter_round!(output[0], output[4], output[8], output[12]); 
        quarter_round!(output[1], output[5], output[9], output[13]); 
        quarter_round!(output[2], output[6], output[10], output[14]); 
        quarter_round!(output[3], output[7], output[11], output[15]); 

        
        quarter_round!(output[0], output[5], output[10], output[15]); 
        quarter_round!(output[1], output[6], output[11], output[12]); 
        quarter_round!(output[2], output[7], output[8], output[13]); 
        quarter_round!(output[3], output[4], output[9], output[14]); 
    }
    for (a, &b) in output.iter_mut().zip(input.iter()) {
        *a = a.wrapping_add(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    fn output_hex(inp: &[u32; 16]) -> String {
        let mut res = String::new();
        res.reserve(512 / 4);
        for &x in inp {
            write!(&mut res, "{x:08x}").unwrap();
        }
        res
    }

    #[test]
    
    fn basic_tv1() {
        let mut inp = [0u32; 16];
        let mut out = [0u32; 16];
        inp[0] = C[0];
        inp[1] = C[1];
        inp[2] = C[2];
        inp[3] = C[3];
        inp[4] = 0x03020100; 
        inp[5] = 0x07060504;
        inp[6] = 0x0b0a0908;
        inp[7] = 0x0f0e0d0c;
        inp[8] = 0x13121110;
        inp[9] = 0x17161514;
        inp[10] = 0x1b1a1918;
        inp[11] = 0x1f1e1d1c;
        inp[12] = 0x00000001; 
        inp[13] = 0x09000000; 
        inp[14] = 0x4a000000; 
        inp[15] = 0x00000000; 
        chacha20(&inp, &mut out);
        assert_eq!(
            output_hex(&out),
            concat!(
                "e4e7f11015593bd11fdd0f50c47120a3c7f4d1c70368c0339aaa22044e6cd4c3",
                "466482d209aa9f0705d7c214a2028bd9d19c12b5b94e16dee883d0cb4e3c50a2"
            )
        );
    }
}
