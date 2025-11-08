macro_rules! quarter_round {
    ($v1:expr,$v2:expr,$v3:expr,$v4:expr) => {
        $v2 ^= ($v1.wrapping_add($v4).rotate_left(7));
        $v3 ^= ($v2.wrapping_add($v1).rotate_left(9));
        $v4 ^= ($v3.wrapping_add($v2).rotate_left(13));
        $v1 ^= ($v4.wrapping_add($v3).rotate_left(18));
    };
}




















































pub fn salsa20(input: &[u32; 16], output: &mut [u32; 16]) {
    output.copy_from_slice(&input[..]);
    for _ in 0..10 {
        
        quarter_round!(output[0], output[4], output[8], output[12]); 
        quarter_round!(output[5], output[9], output[13], output[1]); 
        quarter_round!(output[10], output[14], output[2], output[6]); 
        quarter_round!(output[15], output[3], output[7], output[11]); 

        
        quarter_round!(output[0], output[1], output[2], output[3]); 
        quarter_round!(output[5], output[6], output[7], output[4]); 
        quarter_round!(output[10], output[11], output[8], output[9]); 
        quarter_round!(output[15], output[12], output[13], output[14]); 
    }
    for (a, &b) in output.iter_mut().zip(input.iter()) {
        *a = a.wrapping_add(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    const C: [u32; 4] = [0x65787061, 0x6e642033, 0x322d6279, 0x7465206b];

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
        inp[1] = 0x01020304; 
        inp[2] = 0x05060708; 
        inp[3] = 0x090a0b0c;
        inp[4] = 0x0d0e0f10;
        inp[5] = C[1];
        inp[6] = 0x65666768; 
        inp[7] = 0x696a6b6c; 
        inp[8] = 0x6d6e6f70;
        inp[9] = 0x71727374;
        inp[10] = C[2];
        inp[11] = 0xc9cacbcc; 
        inp[12] = 0xcdcecfd0; 
        inp[13] = 0xd1d2d3d4;
        inp[14] = 0xd5d6d7d8;
        inp[15] = C[3];
        salsa20(&inp, &mut out);
        
        
        assert_eq!(
            output_hex(&out),
            concat!(
                "de1d6f8d91dbf69d0db4b70c8b4320d236694432896d98b05aa7b76d5738ca13",
                "04e5a170c8e479af1542ed2f30f26ba57da20203cfe955c66f4cc7a06dd34359"
            )
        );
    }
}
