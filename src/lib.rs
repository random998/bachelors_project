use num_bigint::BigInt;

fn main() {
}

/// computes the extended eudclidean algorithhm, given signed integers a,b.
fn extended_gcd(a: i64, b: i64) -> (i64, i64, i64) {
    let (mut old_r, mut r) = (a,b);
    let (mut old_s, mut s) = (1,0);
    let (mut old_t, mut t) = (0,1);

    while r != 0i64 {
        let q = old_r / r;
        (old_r, r) = (r, old_r - q * r);
        (old_s, s) = (s, old_s - q * s);
        (old_t, t) = (t, old_t - q * t);
    }
    (old_s, old_t, old_r)
}

/// computes the extended eudclidean algorithhm, given signed integers a,b.
fn big_extended_gcd(a: BigInt, b: BigInt) -> (BigInt, BigInt, BigInt) {
    let (mut old_r, mut r) = (a,b);
    let (mut old_s, mut s) = (BigInt::from(1), BigInt::ZERO);
    let (mut old_t, mut t) = (BigInt::ZERO ,BigInt::from(1));

    while r != BigInt::ZERO {
        let q = old_r.clone() / r.clone();
        (old_r, r) = (r.clone(), old_r.clone()- q.clone() * r.clone());
        (old_s, s) = (s.clone(), old_s.clone()- q.clone() * s.clone());
        (old_t, t) = (t.clone(), old_t.clone() - q.clone() * t.clone());
    }
    (old_s, old_t, old_r)
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;
    use num_bigint::BigInt;
    use super::big_extended_gcd;
    use super::extended_gcd;

    quickcheck! {
        fn prop_big_extended_gcd(a: BigInt, b: BigInt) -> bool {
            let (x,y,gcd) = big_extended_gcd(a.clone(), b.clone());
            gcd == a.clone() * x + b.clone() * y
        }

        fn prop_extended_gcd(a: i32, b: i32) -> bool {
            let (x,y,gcd) = extended_gcd(a as i64, b as i64);
            gcd == a as i64 * x + b as i64 * y
            
        }
    }
}

