//! # Zero-Knowledge Range Proof (Bulletproofs over Ed25519)
//!
//! Implements a self-contained ZK range proof scheme to prove that a score $v$
//! committed in a Pedersen commitment $C = g^v h^r$ is below a threshold $T$,
//! i.e. $v \in [0, T)$.
//!
//! Since $v \in [0, 100]$ is guaranteed by score validation, we prove $v < T$ by
//! proving that $T - 1 - v \ge 0$, i.e. $T - 1 - v \in [0, 2^8)$.
//!
//! We verify this by computing $C' = g^{T-1} C^{-1}$ and verifying a Bulletproof
//! showing that $C'$ commits to a value in $[0, 2^8)$.

#![allow(non_snake_case)]

use soroban_sdk::{Bytes, BytesN, Env};
#[cfg(test)]
extern crate std;

// ── Field Element Arithmetic ──────────────────────────────────────────────────
//
// 256-bit big-integer arithmetic modulo p = 2^255 - 19 (Curve25519 prime).
// Represented as 4 little-endian u64 limbs.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fe(pub [u64; 4]);

impl Fe {
    pub const P: Fe = Fe([
        0xffffffffffffffed,
        0xffffffffffffffff,
        0xffffffffffffffff,
        0x7fffffffffffffff,
    ]);

    pub fn zero() -> Self {
        Fe([0; 4])
    }

    pub fn one() -> Self {
        Fe([1, 0, 0, 0])
    }

    pub fn from_u64(val: u64) -> Self {
        Fe([val, 0, 0, 0])
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0; 4]
    }

    pub fn ge(&self, other: &Self) -> bool {
        for i in (0..4).rev() {
            if self.0[i] > other.0[i] {
                return true;
            } else if self.0[i] < other.0[i] {
                return false;
            }
        }
        true
    }

    pub fn add(self, other: Self) -> Self {
        let mut res = [0u64; 4];
        let mut carry = 0u128;
        for i in 0..4 {
            let sum = (self.0[i] as u128) + (other.0[i] as u128) + carry;
            res[i] = sum as u64;
            carry = sum >> 64;
        }
        let h = (res[3] >> 63) + (carry << 1) as u64;
        res[3] &= 0x7fffffffffffffff;
        
        let mut carry2 = (h as u128) * 19;
        for i in 0..4 {
            let sum = (res[i] as u128) + carry2;
            res[i] = sum as u64;
            carry2 = sum >> 64;
        }
        let mut out = Fe(res);
        if out.ge(&Self::P) {
            out = out.sub(Self::P);
        }
        out
    }

    pub fn sub(self, other: Self) -> Self {
        let mut res = [0u64; 4];
        let mut borrow = 0u128;
        for i in 0..4 {
            let diff = (self.0[i] as u128).wrapping_sub(other.0[i] as u128).wrapping_sub(borrow);
            res[i] = diff as u64;
            borrow = if diff >> 64 > 0 { 1 } else { 0 };
        }
        if borrow > 0 {
            let mut carry = 0u128;
            for i in 0..4 {
                let sum = (res[i] as u128) + (Self::P.0[i] as u128) + carry;
                res[i] = sum as u64;
                carry = sum >> 64;
            }
        }
        Fe(res)
    }

    pub fn neg(self) -> Self {
        Self::zero().sub(self)
    }

    pub fn mul(self, other: Self) -> Self {
        let mut prod = [0u64; 8];
        for i in 0..4 {
            let mut carry = 0u128;
            for j in 0..4 {
                let product = (self.0[i] as u128) * (other.0[j] as u128) + (prod[i + j] as u128) + carry;
                prod[i + j] = product as u64;
                carry = product >> 64;
            }
            prod[i + 4] = carry as u64;
        }

        let mut l = [0u64; 4];
        l[0] = prod[0];
        l[1] = prod[1];
        l[2] = prod[2];
        l[3] = prod[3] & 0x7fffffffffffffff;

        let mut h = [0u64; 5];
        h[0] = (prod[3] >> 63) | (prod[4] << 1);
        h[1] = (prod[4] >> 63) | (prod[5] << 1);
        h[2] = (prod[5] >> 63) | (prod[6] << 1);
        h[3] = (prod[6] >> 63) | (prod[7] << 1);
        h[4] = prod[7] >> 63;

        let mut carry_mul = 0u128;
        let mut res = l;
        for i in 0..4 {
            let term = (h[i] as u128) * 19 + carry_mul + (res[i] as u128);
            res[i] = term as u64;
            carry_mul = term >> 64;
        }
        let carry_mul2 = (h[4] as u128) * 19 + carry_mul;

        let h2 = (res[3] >> 63) + (carry_mul2 << 1) as u64;
        res[3] &= 0x7fffffffffffffff;

        let mut carry3 = (h2 as u128) * 19;
        for i in 0..4 {
            let sum = (res[i] as u128) + carry3;
            res[i] = sum as u64;
            carry3 = sum >> 64;
        }

        let mut out = Fe(res);
        while out.ge(&Self::P) {
            out = out.sub(Self::P);
        }
        out
    }

    pub fn pow(self, exp: Self) -> Self {
        let mut res = Self::one();
        let mut base = self;
        let mut e = exp;
        while !e.is_zero() {
            if e.0[0] & 1 == 1 {
                res = res.mul(base);
            }
            base = base.mul(base);
            e = e.shr1();
        }
        res
    }

    pub fn shr1(self) -> Self {
        let mut res = [0u64; 4];
        let mut carry = 0u64;
        for i in (0..4).rev() {
            res[i] = (self.0[i] >> 1) | carry;
            carry = self.0[i] << 63;
        }
        Fe(res)
    }

    pub fn invert(self) -> Self {
        let pm2 = Fe([
            0xffffffffffffffeb,
            0xffffffffffffffff,
            0xffffffffffffffff,
            0x7fffffffffffffff,
        ]);
        self.pow(pm2)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for i in 0..4 {
            out[i*8 .. i*8 + 8].copy_from_slice(&self.0[i].to_le_bytes());
        }
        out
    }
}

// ── Scalar Arithmetic (modulo group order l) ─────────────────────────────────
//
// 256-bit big-integer arithmetic modulo l = 2^252 + 27742317777372353535851937790883648493.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sc(pub [u64; 4]);

impl Sc {
    pub const L: Sc = Sc([
        0x5812631a5cf5d3ed,
        0x14def9dea2f79cd6,
        0x0,
        0x1000000000000000,
    ]);

    pub const DIFF: Sc = Sc([
        0xa7ed9ce5a30a2c13,
        0xeb2106215d086329,
        0xffffffffffffffff,
        0xefffffffffffffff,
    ]);

    pub fn zero() -> Self {
        Sc([0; 4])
    }

    pub fn one() -> Self {
        Sc([1, 0, 0, 0])
    }

    pub fn from_u64(val: u64) -> Self {
        Sc([val, 0, 0, 0])
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0; 4]
    }

    pub fn ge(&self, other: &Self) -> bool {
        for i in (0..4).rev() {
            if self.0[i] > other.0[i] {
                return true;
            } else if self.0[i] < other.0[i] {
                return false;
            }
        }
        true
    }

    pub fn add(self, other: Self) -> Self {
        let mut res = [0u64; 4];
        let mut carry = 0u128;
        for i in 0..4 {
            let sum = (self.0[i] as u128) + (other.0[i] as u128) + carry;
            res[i] = sum as u64;
            carry = sum >> 64;
        }
        let mut out = Sc(res);
        if carry > 0 || out.ge(&Self::L) {
            out = out.sub(Self::L);
        }
        out
    }

    pub fn sub(self, other: Self) -> Self {
        let mut res = [0u64; 4];
        let mut borrow = 0u128;
        for i in 0..4 {
            let diff = (self.0[i] as u128).wrapping_sub(other.0[i] as u128).wrapping_sub(borrow);
            res[i] = diff as u64;
            borrow = if diff >> 64 > 0 { 1 } else { 0 };
        }
        if borrow > 0 {
            let mut carry = 0u128;
            for i in 0..4 {
                let sum = (res[i] as u128) + (Self::L.0[i] as u128) + carry;
                res[i] = sum as u64;
                carry = sum >> 64;
            }
        }
        Sc(res)
    }

    pub fn neg(self) -> Self {
        Self::zero().sub(self)
    }

    pub fn mul(self, other: Self) -> Self {
        let mut prod = [0u64; 8];
        for i in 0..4 {
            let mut carry = 0u128;
            for j in 0..4 {
                let product = (self.0[i] as u128) * (other.0[j] as u128) + (prod[i + j] as u128) + carry;
                prod[i + j] = product as u64;
                carry = product >> 64;
            }
            prod[i + 4] = carry as u64;
        }

        let mut r = [0u64; 4];
        for i in (0..512).rev() {
            let mut carry_r = 0u64;
            for j in 0..4 {
                let next_carry = r[j] >> 63;
                r[j] = (r[j] << 1) | carry_r;
                carry_r = next_carry;
            }
            let limb_idx = i / 64;
            let bit_idx = i % 64;
            let bit = (prod[limb_idx] >> bit_idx) & 1;
            r[0] |= bit;

            let mut r_sc = Sc(r);
            if carry_r > 0 {
                r_sc = r_sc.add(Self::DIFF);
            } else if r_sc.ge(&Self::L) {
                r_sc = r_sc.sub(Self::L);
            }
            r = r_sc.0;
        }
        Sc(r)
    }

    pub fn pow(self, exp: Self) -> Self {
        let mut res = Self::one();
        let mut base = self;
        let mut e = exp;
        while !e.is_zero() {
            if e.0[0] & 1 == 1 {
                res = res.mul(base);
            }
            base = base.mul(base);
            e = e.shr1();
        }
        res
    }

    pub fn shr1(self) -> Self {
        let mut res = [0u64; 4];
        let mut carry = 0u64;
        for i in (0..4).rev() {
            res[i] = (self.0[i] >> 1) | carry;
            carry = self.0[i] << 63;
        }
        Sc(res)
    }

    pub fn invert(self) -> Self {
        let lm2 = Self::L.sub(Self::from_u64(2));
        self.pow(lm2)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for i in 0..4 {
            out[i*8 .. i*8 + 8].copy_from_slice(&self.0[i].to_le_bytes());
        }
        out
    }
}

pub fn compress_pt(env: &Env, pt: &Pt) -> BytesN<32> {
    let mut out = [0u8; 32];
    let y_bytes = pt.y.to_bytes();
    out.copy_from_slice(&y_bytes);
    let x_lsb = (pt.x.0[0] & 1) as u8;
    out[31] = (out[31] & 0x7f) | (x_lsb << 7);
    BytesN::from_array(env, &out)
}

pub fn decompress_pt_32(env: &Env, bytes: &BytesN<32>) -> Option<Pt> {
    let arr = bytes.to_array();
    let mut y_bytes = arr;
    let sign = y_bytes[31] >> 7;
    y_bytes[31] &= 0x7f;
    
    let mut y_limbs = [0u64; 4];
    for i in 0..4 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&y_bytes[i*8 .. i*8 + 8]);
        y_limbs[i] = u64::from_le_bytes(b);
    }
    let y = Fe(y_limbs);
    if y.ge(&Fe::P) {
        return None;
    }
    
    let y2 = y.mul(y);
    let u = y2.sub(Fe::one());
    let d = Fe::from_u64(121665).neg().mul(Fe::from_u64(121666).invert());
    let v = d.mul(y2).add(Fe::one());
    
    let v2 = v.mul(v);
    let v3 = v2.mul(v);
    let v7 = v3.mul(v3).mul(v);
    let uv7 = u.mul(v7);
    
    let exp = Fe([
        0xfffffffffffffffd,
        0xffffffffffffffff,
        0xffffffffffffffff,
        0x0fffffffffffffff,
    ]);
    let uv7_exp = uv7.pow(exp);
    let mut x = u.mul(v3).mul(uv7_exp);
    
    let vx2 = v.mul(x.mul(x));
    if vx2 != u {
        let i_val = Fe([
            0xc4ee1b274a0ea0b0,
            0x2f431806ad2fe478,
            0x2b4d00993dfbd7a7,
            0x2b8324804fc1df0b,
        ]);
        x = x.mul(i_val);
        let vx2_i = v.mul(x.mul(x));
        if vx2_i != u {
            return None;
        }
    }
    
    let x_lsb = (x.0[0] & 1) as u8;
    if x_lsb != sign {
        x = Fe::zero().sub(x);
    }
    
    Some(Pt { x, y })
}

// ── Twisted Edwards Curve Arithmetic (Ed25519) ────────────────────────────────
//
// Curve equation: -x^2 + y^2 = 1 + d x^2 y^2 mod p

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pt {
    pub x: Fe,
    pub y: Fe,
}

impl Pt {
    pub fn identity() -> Self {
        Pt {
            x: Fe::zero(),
            y: Fe::one(),
        }
    }

    pub fn is_identity(&self) -> bool {
        self.x.is_zero() && self.y == Fe::one()
    }

    pub fn add(self, other: Self, d: Fe) -> Self {
        if self.is_identity() {
            return other;
        }
        if other.is_identity() {
            return self;
        }
        let x1x2 = self.x.mul(other.x);
        let y1y2 = self.y.mul(other.y);
        let x1y2 = self.x.mul(other.y);
        let y1x2 = self.y.mul(other.x);
        let dx1x2y1y2 = d.mul(x1x2).mul(y1y2);

        let num_x = x1y2.add(y1x2);
        let den_x = Fe::one().add(dx1x2y1y2);
        let x3 = num_x.mul(den_x.invert());

        let num_y = y1y2.add(x1x2);
        let den_y = Fe::one().sub(dx1x2y1y2);
        let y3 = num_y.mul(den_y.invert());

        Pt { x: x3, y: y3 }
    }

    pub fn double(self, d: Fe) -> Self {
        self.add(self, d)
    }

    pub fn mul(self, scalar: Sc, d: Fe) -> Self {
        let mut res = Self::identity();
        let mut temp = self;
        let mut s = scalar;
        while !s.is_zero() {
            if s.0[0] & 1 == 1 {
                res = res.add(temp, d);
            }
            temp = temp.double(d);
            s = s.shr1();
        }
        res
    }
}

pub fn is_on_curve(x: Fe, y: Fe, d: Fe) -> bool {
    let x2 = x.mul(x);
    let y2 = y.mul(y);
    let lhs = y2.sub(x2);
    let rhs = Fe::one().add(d.mul(x2).mul(y2));
    lhs == rhs
}

// ── Generators ────────────────────────────────────────────────────────────────

pub fn g() -> Pt {
    Pt {
        x: Fe([
            0xc9562d608f25d51a,
            0x692cc7609525a7b2,
            0xc0a4e231fdd6dc5c,
            0x216936d3cd6e53fe,
        ]),
        y: Fe([
            0x6666666666666658,
            0x6666666666666666,
            0x6666666666666666,
            0x6666666666666666,
        ]),
    }
}

pub fn get_generators() -> (Pt, Pt, Fe) {
    let d = Fe::from_u64(121665).neg().mul(Fe::from_u64(121666).invert());
    let g_pt = g();
    let h_pt = g_pt.mul(Sc::from_u64(8), d); // independent generator H = 8G
    (g_pt, h_pt, d)
}

pub fn get_vector_generators(d: Fe) -> ([Pt; 8], [Pt; 8]) {
    let mut gs = [Pt::identity(); 8];
    let mut hs = [Pt::identity(); 8];
    for i in 0..8 {
        gs[i] = g().mul(Sc::from_u64((16 + i) as u64), d);
        hs[i] = g().mul(Sc::from_u64((32 + i) as u64), d);
    }
    (gs, hs)
}

// ── Bulletproof Struct & Serialization ────────────────────────────────────────

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bulletproof {
    pub A: Pt,
    pub S: Pt,
    pub T1: Pt,
    pub T2: Pt,
    pub tx: Sc,
    pub taux: Sc,
    pub mu: Sc,
    pub L: [Pt; 3],
    pub R: [Pt; 3],
    pub a: Sc,
    pub b: Sc,
}

impl Bulletproof {
    pub fn to_bytes(&self, env: &Env) -> Bytes {
        let mut buf = [0u8; 800];
        let mut offset = 0;
        
        let pts = [&self.A, &self.S, &self.T1, &self.T2];
        for p in pts {
            buf[offset..offset+32].copy_from_slice(&p.x.to_bytes());
            buf[offset+32..offset+64].copy_from_slice(&p.y.to_bytes());
            offset += 64;
        }
        
        let scs = [&self.tx, &self.taux, &self.mu];
        for s in scs {
            buf[offset..offset+32].copy_from_slice(&s.to_bytes());
            offset += 32;
        }
        
        for p in &self.L {
            buf[offset..offset+32].copy_from_slice(&p.x.to_bytes());
            buf[offset+32..offset+64].copy_from_slice(&p.y.to_bytes());
            offset += 64;
        }
        
        for p in &self.R {
            buf[offset..offset+32].copy_from_slice(&p.x.to_bytes());
            buf[offset+32..offset+64].copy_from_slice(&p.y.to_bytes());
            offset += 64;
        }
        
        let scs2 = [&self.a, &self.b];
        for s in scs2 {
            buf[offset..offset+32].copy_from_slice(&s.to_bytes());
            offset += 32;
        }
        
        Bytes::from_array(env, &buf)
    }

    pub fn from_bytes(bytes: &Bytes) -> Option<Self> {
        if bytes.len() != 800 {
            return None;
        }
        
        let mut offset = 0;
        let read_pt = |bytes: &Bytes, offset: &mut u32| -> Option<Pt> {
            let mut x_limbs = [0u64; 4];
            let mut y_limbs = [0u64; 4];
            for i in 0..4 {
                let mut b = [0u8; 8];
                for j in 0..8 {
                    b[j] = bytes.get(*offset + (i as u32)*8 + j as u32)?;
                }
                x_limbs[i as usize] = u64::from_le_bytes(b);
            }
            *offset += 32;
            for i in 0..4 {
                let mut b = [0u8; 8];
                for j in 0..8 {
                    b[j] = bytes.get(*offset + (i as u32)*8 + j as u32)?;
                }
                y_limbs[i as usize] = u64::from_le_bytes(b);
            }
            *offset += 32;
            let x = Fe(x_limbs);
            let y = Fe(y_limbs);
            let d = Fe::from_u64(121665).neg().mul(Fe::from_u64(121666).invert());
            if is_on_curve(x, y, d) {
                Some(Pt { x, y })
            } else {
                None
            }
        };
        
        let read_sc = |bytes: &Bytes, offset: &mut u32| -> Option<Sc> {
            let mut limbs = [0u64; 4];
            for i in 0..4 {
                let mut b = [0u8; 8];
                for j in 0..8 {
                    b[j] = bytes.get(*offset + (i as u32)*8 + j as u32)?;
                }
                limbs[i as usize] = u64::from_le_bytes(b);
            }
            *offset += 32;
            Some(Sc(limbs))
        };
        
        let A = read_pt(bytes, &mut offset)?;
        let S = read_pt(bytes, &mut offset)?;
        let T1 = read_pt(bytes, &mut offset)?;
        let T2 = read_pt(bytes, &mut offset)?;
        
        let tx = read_sc(bytes, &mut offset)?;
        let taux = read_sc(bytes, &mut offset)?;
        let mu = read_sc(bytes, &mut offset)?;
        
        let mut L = [Pt::identity(); 3];
        for i in 0..3 {
            L[i] = read_pt(bytes, &mut offset)?;
        }
        
        let mut R = [Pt::identity(); 3];
        for i in 0..3 {
            R[i] = read_pt(bytes, &mut offset)?;
        }
        
        let a = read_sc(bytes, &mut offset)?;
        let b = read_sc(bytes, &mut offset)?;
        
        Some(Bulletproof {
            A,
            S,
            T1,
            T2,
            tx,
            taux,
            mu,
            L,
            R,
            a,
            b,
        })
    }
}

// ── Fiat-Shamir Hashing ───────────────────────────────────────────────────────

fn hash_to_scalar(env: &Env, data: &Bytes) -> Sc {
    let hash = env.crypto().sha256(data);
    let arr = hash.to_array();
    let mut limbs = [0u64; 4];
    for i in 0..4 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&arr[i*8 .. i*8 + 8]);
        limbs[i] = u64::from_le_bytes(b);
    }
    let mut val = Sc(limbs);
    val.0[3] &= 0x0fffffffffffffff;
    while val.ge(&Sc::L) {
        val = val.sub(Sc::L);
    }
    val
}

fn append_pt(env: &Env, data: &mut Bytes, pt: &Pt) {
    let xb = pt.x.to_bytes();
    let yb = pt.y.to_bytes();
    data.append(&Bytes::from_array(env, &xb));
    data.append(&Bytes::from_array(env, &yb));
}

fn append_sc(env: &Env, data: &mut Bytes, sc: &Sc) {
    let sb = sc.to_bytes();
    data.append(&Bytes::from_array(env, &sb));
}

fn hash_fs_y_z(env: &Env, V: &Pt, A: &Pt, S: &Pt) -> (Sc, Sc) {
    let mut data = Bytes::new(env);
    data.append(&Bytes::from_array(env, b"y_z"));
    append_pt(env, &mut data, V);
    append_pt(env, &mut data, A);
    append_pt(env, &mut data, S);
    let y = hash_to_scalar(env, &data);
    
    let mut data2 = Bytes::new(env);
    data2.append(&Bytes::from_array(env, b"z"));
    append_sc(env, &mut data2, &y);
    let z = hash_to_scalar(env, &data2);
    
    (y, z)
}

fn hash_fs_x(env: &Env, T1: &Pt, T2: &Pt) -> Sc {
    let mut data = Bytes::new(env);
    data.append(&Bytes::from_array(env, b"x"));
    append_pt(env, &mut data, T1);
    append_pt(env, &mut data, T2);
    hash_to_scalar(env, &data)
}

fn hash_fs_w(env: &Env, tx: &Sc, taux: &Sc, mu: &Sc) -> Sc {
    let mut data = Bytes::new(env);
    data.append(&Bytes::from_array(env, b"w"));
    append_sc(env, &mut data, tx);
    append_sc(env, &mut data, taux);
    append_sc(env, &mut data, mu);
    hash_to_scalar(env, &data)
}

fn hash_fs_challenge_ip(env: &Env, round: usize, L: &Pt, R: &Pt) -> Sc {
    let mut data = Bytes::new(env);
    data.append(&Bytes::from_array(env, b"ip"));
    let r_byte = [round as u8];
    data.append(&Bytes::from_array(env, &r_byte));
    append_pt(env, &mut data, L);
    append_pt(env, &mut data, R);
    hash_to_scalar(env, &data)
}

// ── Bulletproof Prover ────────────────────────────────────────────────────────

pub struct SeededPrng {
    state: [u8; 32],
}

impl SeededPrng {
    pub fn new(seed: [u8; 32]) -> Self {
        SeededPrng { state: seed }
    }

    pub fn next_scalar(&mut self, env: &Env) -> Sc {
        let mut buf = [0u8; 64];
        buf[0..32].copy_from_slice(&self.state);
        for i in 0..32 {
            self.state[i] = self.state[i].wrapping_add(1);
        }
        let hash = env.crypto().sha256(&Bytes::from_array(env, &buf));
        let arr = hash.to_array();
        let mut limbs = [0u64; 4];
        for i in 0..4 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&arr[i*8 .. i*8 + 8]);
            limbs[i] = u64::from_le_bytes(b);
        }
        let mut val = Sc(limbs);
        val.0[3] &= 0x0fffffffffffffff;
        while val.ge(&Sc::L) {
            val = val.sub(Sc::L);
        }
        val
    }
}

pub fn prove_range_proof(env: &Env, v: u32, r: Sc, mut prng: SeededPrng) -> Bulletproof {
    let (g_pt, h_pt, d) = get_generators();
    let (gs, hs) = get_vector_generators(d);
    
    let mut a_L = [Sc::zero(); 8];
    let mut a_R = [Sc::zero(); 8];
    for i in 0..8 {
        let bit = ((v >> i) & 1) as u64;
        a_L[i] = Sc::from_u64(bit);
        a_R[i] = Sc::from_u64(bit).sub(Sc::one());
    }
    
    let alpha = prng.next_scalar(env);
    let mut A = h_pt.mul(alpha, d);
    for i in 0..8 {
        A = A.add(gs[i].mul(a_L[i], d), d).add(hs[i].mul(a_R[i], d), d);
    }
    
    let mut s_L = [Sc::zero(); 8];
    let mut s_R = [Sc::zero(); 8];
    for i in 0..8 {
        s_L[i] = prng.next_scalar(env);
        s_R[i] = prng.next_scalar(env);
    }
    let beta = prng.next_scalar(env);
    let mut S = h_pt.mul(beta, d);
    for i in 0..8 {
        S = S.add(gs[i].mul(s_L[i], d), d).add(hs[i].mul(s_R[i], d), d);
    }
    
    let V = g_pt.mul(Sc::from_u64(v as u64), d).add(h_pt.mul(r, d), d);
    let (y, z) = hash_fs_y_z(env, &V, &A, &S);
    #[cfg(test)]
    {
        std::println!("PROVER: V.x = {:?}, V.y = {:?}", V.x, V.y);
        std::println!("PROVER: A.x = {:?}, A.y = {:?}", A.x, A.y);
        std::println!("PROVER: S.x = {:?}, S.y = {:?}", S.x, S.y);
        std::println!("PROVER: y = {:?}, z = {:?}", y, z);
    }
    
    let mut y_pow = [Sc::one(); 8];
    for i in 1..8 {
        y_pow[i] = y_pow[i-1].mul(y);
    }
    
    let z2 = z.mul(z);
    let z3 = z2.mul(z);
    
    let mut t1 = Sc::zero();
    let mut t2 = Sc::zero();
    for i in 0..8 {
        let l_0 = a_L[i].sub(z);
        let l_1 = s_L[i];
        
        let two_pow_i = Sc::from_u64(1 << i);
        let r_0 = y_pow[i].mul(a_R[i].add(z)).add(z2.mul(two_pow_i));
        let r_1 = y_pow[i].mul(s_R[i]);
        
        t1 = t1.add(l_0.mul(r_1).add(l_1.mul(r_0)));
        t2 = t2.add(l_1.mul(r_1));
    }
    
    let tau1 = prng.next_scalar(env);
    let tau2 = prng.next_scalar(env);
    let T1 = g_pt.mul(t1, d).add(h_pt.mul(tau1, d), d);
    let T2 = g_pt.mul(t2, d).add(h_pt.mul(tau2, d), d);
    
    let x = hash_fs_x(env, &T1, &T2);
    #[cfg(test)]
    {
        std::println!("PROVER: T1.x = {:?}, T1.y = {:?}", T1.x, T1.y);
        std::println!("PROVER: T2.x = {:?}, T2.y = {:?}", T2.x, T2.y);
        std::println!("PROVER: x = {:?}", x);
    }
    let x2 = x.mul(x);
    
    let mut l = [Sc::zero(); 8];
    let mut r_vec = [Sc::zero(); 8];
    let mut tx = Sc::zero();
    for i in 0..8 {
        l[i] = a_L[i].sub(z).add(s_L[i].mul(x));
        let two_pow_i = Sc::from_u64(1 << i);
        r_vec[i] = y_pow[i].mul(a_R[i].add(z).add(s_R[i].mul(x))).add(z2.mul(two_pow_i));
        tx = tx.add(l[i].mul(r_vec[i]));
    }
    
    let taux = tau2.mul(x2).add(tau1.mul(x)).add(z2.mul(r));
    let mu = alpha.add(beta.mul(x));
    
    let w = hash_fs_w(env, &tx, &taux, &mu);
    let Q = g_pt.mul(w, d);
    
    let mut gs_ip = gs;
    let mut hs_ip = hs;
    let y_inv = y.invert();
    let mut y_inv_pow = Sc::one();
    for i in 0..8 {
        hs_ip[i] = hs_ip[i].mul(y_inv_pow, d);
        y_inv_pow = y_inv_pow.mul(y_inv);
    }
    
    let mut ip_l = l;
    let mut ip_r = r_vec;
    
    let mut L = [Pt::identity(); 3];
    let mut R = [Pt::identity(); 3];
    
    let mut len = 8;
    for round in 0..3 {
        let half = len / 2;
        let mut c_L = Sc::zero();
        let mut c_R = Sc::zero();
        for i in 0..half {
            c_L = c_L.add(ip_l[i].mul(ip_r[half + i]));
            c_R = c_R.add(ip_l[half + i].mul(ip_r[i]));
        }
        
        let mut L_pt = Q.mul(c_L, d);
        for i in 0..half {
            L_pt = L_pt.add(gs_ip[half + i].mul(ip_l[i], d), d)
                      .add(hs_ip[i].mul(ip_r[half + i], d), d);
        }
        
        let mut R_pt = Q.mul(c_R, d);
        for i in 0..half {
            R_pt = R_pt.add(gs_ip[i].mul(ip_l[half + i], d), d)
                      .add(hs_ip[half + i].mul(ip_r[i], d), d);
        }
        
        L[round] = L_pt;
        R[round] = R_pt;
        
        let u = hash_fs_challenge_ip(env, round, &L[round], &R[round]);
        let u_inv = u.invert();
        
        for i in 0..half {
            ip_l[i] = ip_l[i].mul(u).add(ip_l[half + i].mul(u_inv));
            ip_r[i] = ip_r[i].mul(u_inv).add(ip_r[half + i].mul(u));
        }
        
        for i in 0..half {
            gs_ip[i] = gs_ip[i].mul(u_inv, d).add(gs_ip[half + i].mul(u, d), d);
            hs_ip[i] = hs_ip[i].mul(u, d).add(hs_ip[half + i].mul(u_inv, d), d);
        }
        
        len = half;
    }
    
    Bulletproof {
        A,
        S,
        T1,
        T2,
        tx,
        taux,
        mu,
        L,
        R,
        a: ip_l[0],
        b: ip_r[0],
    }
}

// ── Bulletproof Verifier ──────────────────────────────────────────────────────

pub fn verify_range_proof(env: &Env, V: Pt, proof: &Bulletproof) -> bool {
    let (g_pt, h_pt, d) = get_generators();
    let (gs, hs) = get_vector_generators(d);
    
    let (y, z) = hash_fs_y_z(env, &V, &proof.A, &proof.S);
    let x = hash_fs_x(env, &proof.T1, &proof.T2);
    #[cfg(test)]
    {
        std::println!("VERIFIER: V.x = {:?}, V.y = {:?}", V.x, V.y);
        std::println!("VERIFIER: A.x = {:?}, A.y = {:?}", proof.A.x, proof.A.y);
        std::println!("VERIFIER: S.x = {:?}, S.y = {:?}", proof.S.x, proof.S.y);
        std::println!("VERIFIER: y = {:?}, z = {:?}", y, z);
        std::println!("VERIFIER: T1.x = {:?}, T1.y = {:?}", proof.T1.x, proof.T1.y);
        std::println!("VERIFIER: T2.x = {:?}, T2.y = {:?}", proof.T2.x, proof.T2.y);
        std::println!("VERIFIER: x = {:?}", x);
    }
    
    let z2 = z.mul(z);
    let z3 = z2.mul(z);
    let x2 = x.mul(x);
    
    let mut sum_y = Sc::zero();
    let mut sum_two = Sc::zero();
    let mut y_pow = Sc::one();
    for i in 0..8 {
        sum_y = sum_y.add(y_pow);
        sum_two = sum_two.add(Sc::from_u64(1 << i));
        y_pow = y_pow.mul(y);
    }
    let delta = z.sub(z2).mul(sum_y).sub(z3.mul(sum_two));
    
    let lhs1 = g_pt.mul(proof.tx, d).add(h_pt.mul(proof.taux, d), d);
    let rhs1 = V.mul(z2, d)
        .add(g_pt.mul(delta, d), d)
        .add(proof.T1.mul(x, d), d)
        .add(proof.T2.mul(x2, d), d);
        
    if lhs1 != rhs1 {
        #[cfg(test)]
        std::println!("verify_range_proof: lhs1 == rhs1 check failed!");
        return false;
    }
    
    let w = hash_fs_w(env, &proof.tx, &proof.taux, &proof.mu);
    let Q = g_pt.mul(w, d);
    
    let mut P = proof.A.add(proof.S.mul(x, d), d).add(h_pt.mul(proof.mu.neg(), d), d);
    
    let y_inv = y.invert();
    let mut y_inv_pow = Sc::one();
    for i in 0..8 {
        P = P.add(gs[i].mul(z.neg(), d), d);
        let term = z.add(z2.mul(Sc::from_u64(1 << i)).mul(y_inv_pow));
        P = P.add(hs[i].mul(term, d), d);
        y_inv_pow = y_inv_pow.mul(y_inv);
    }
    
    let mut P_prime = P.add(Q.mul(proof.tx, d), d);
    
    let gs_ip = gs;
    let mut hs_ip = hs;
    let mut y_inv_pow = Sc::one();
    for i in 0..8 {
        hs_ip[i] = hs_ip[i].mul(y_inv_pow, d);
        y_inv_pow = y_inv_pow.mul(y_inv);
    }
    
    let mut challenges = [Sc::zero(); 3];
    let mut u_inv_sq = [Sc::zero(); 3];
    let mut u_sq = [Sc::zero(); 3];
    for round in 0..3 {
        let u = hash_fs_challenge_ip(env, round, &proof.L[round], &proof.R[round]);
        challenges[round] = u;
        let u_inv = u.invert();
        u_sq[round] = u.mul(u);
        u_inv_sq[round] = u_inv.mul(u_inv);
        
        P_prime = P_prime.add(proof.L[round].mul(u_sq[round], d), d)
                         .add(proof.R[round].mul(u_inv_sq[round], d), d);
    }
    
    let mut gs_final = Pt::identity();
    let mut hs_final = Pt::identity();
    for i in 0..8 {
        let mut s = Sc::one();
        let mut s_prime = Sc::one();
        for round in 0..3 {
            let bit = (i >> (2 - round)) & 1;
            if bit == 1 {
                s = s.mul(challenges[round]);
                s_prime = s_prime.mul(challenges[round].invert());
            } else {
                s = s.mul(challenges[round].invert());
                s_prime = s_prime.mul(challenges[round]);
            }
        }
        gs_final = gs_final.add(gs_ip[i].mul(s, d), d);
        hs_final = hs_final.add(hs_ip[i].mul(s_prime, d), d);
    }
    
    let ab = proof.a.mul(proof.b);
    let expected = gs_final.mul(proof.a, d)
        .add(hs_final.mul(proof.b, d), d)
        .add(Q.mul(ab, d), d);
        
    let res = P_prime == expected;
    if !res {
        #[cfg(test)]
        std::println!("verify_range_proof: P_prime == expected check failed!");
    }
    res
}
