//! A small vector of plain values with inline storage (ENGINE.md 17): the
//! first `N` elements live in the value itself, more spill into a `Vec`.
//! For the short lists the priority and payment code builds for every
//! decision (mana sources, candidate units, hand slots), where a heap
//! allocation per list was a large part of a decision's cost.

use std::ops::{Deref, DerefMut};

#[derive(Clone)]
pub(crate) struct SVec<T: Copy + Default, const N: usize> {
    /// Elements in `buf` while not spilled.
    len: usize,
    buf: [T; N],
    /// Every element once spilled (then `buf` is unused).
    heap: Vec<T>,
    spilled: bool,
}

impl<T: Copy + Default, const N: usize> SVec<T, N> {
    #[inline]
    pub fn new() -> Self {
        SVec { len: 0, buf: [T::default(); N], heap: Vec::new(), spilled: false }
    }

    #[inline]
    pub fn push(&mut self, x: T) {
        if self.spilled {
            self.heap.push(x);
        } else if self.len < N {
            self.buf[self.len] = x;
            self.len += 1;
        } else {
            let mut v = Vec::with_capacity(2 * N + 1);
            v.extend_from_slice(&self.buf[..self.len]);
            v.push(x);
            self.heap = v;
            self.spilled = true;
        }
    }

    #[inline]
    pub fn truncate(&mut self, n: usize) {
        if self.spilled {
            self.heap.truncate(n);
        } else if n < self.len {
            self.len = n;
        }
    }
}

impl<T: Copy + Default, const N: usize> Default for SVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default, const N: usize> Deref for SVec<T, N> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        if self.spilled {
            &self.heap
        } else {
            &self.buf[..self.len]
        }
    }
}

impl<T: Copy + Default, const N: usize> DerefMut for SVec<T, N> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        if self.spilled {
            &mut self.heap
        } else {
            &mut self.buf[..self.len]
        }
    }
}

impl<T: Copy + Default, const N: usize> Extend<T> for SVec<T, N> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, it: I) {
        for x in it {
            self.push(x);
        }
    }
}

impl<T: Copy + Default, const N: usize> FromIterator<T> for SVec<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(it: I) -> Self {
        let mut v = SVec::new();
        v.extend(it);
        v
    }
}

impl<'a, T: Copy + Default, const N: usize> IntoIterator for &'a SVec<T, N> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// By-value iteration (the elements are `Copy`).
pub(crate) struct IntoIter<T: Copy + Default, const N: usize> {
    v: SVec<T, N>,
    i: usize,
}

impl<T: Copy + Default, const N: usize> Iterator for IntoIter<T, N> {
    type Item = T;
    #[inline]
    fn next(&mut self) -> Option<T> {
        let x = self.v.get(self.i).copied();
        self.i += 1;
        x
    }
}

impl<T: Copy + Default, const N: usize> IntoIterator for SVec<T, N> {
    type Item = T;
    type IntoIter = IntoIter<T, N>;
    fn into_iter(self) -> IntoIter<T, N> {
        IntoIter { v: self, i: 0 }
    }
}

impl<T: Copy + Default + std::fmt::Debug, const N: usize> std::fmt::Debug for SVec<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Copy + Default + PartialEq, const N: usize> PartialEq for SVec<T, N> {
    fn eq(&self, o: &Self) -> bool {
        self[..] == o[..]
    }
}

impl<T: Copy + Default + Eq, const N: usize> Eq for SVec<T, N> {}

#[cfg(test)]
mod tests {
    use super::SVec;

    #[test]
    fn inline_then_spilled() {
        let mut v: SVec<u16, 3> = SVec::new();
        assert!(v.is_empty());
        for i in 0..3 {
            v.push(i);
        }
        assert_eq!(&v[..], &[0, 1, 2]);
        v.push(3);
        v.push(4);
        assert_eq!(&v[..], &[0, 1, 2, 3, 4]);
        v.truncate(2);
        assert_eq!(&v[..], &[0, 1]);
        v[1] = 7;
        assert_eq!(v.iter().copied().collect::<Vec<_>>(), vec![0, 7]);
        let w: SVec<u16, 3> = (0..2).collect();
        assert_eq!(w.len(), 2);
        assert!(w.contains(&1));
        v.truncate(0);
        assert!(v.is_empty());
    }
}
