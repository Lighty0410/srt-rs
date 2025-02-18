use std::iter::Iterator;

use crate::SeqNumber;

/// Iterator for compressing loss lists
pub struct CompressLossList<I> {
    /// Underlying iterator
    iterator: I,

    /// The next item
    next: Option<SeqNumber>,

    /// Some if looping
    last_in_loop: Option<SeqNumber>,
}

impl<I> Iterator for CompressLossList<I>
where
    I: Iterator<Item = SeqNumber>,
{
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        // if we're at the start, assign a next
        if self.next.is_none() {
            self.next = Some(self.iterator.next()?)
        }

        loop {
            // The state right here is
            // neither this nor next have been returned, and we need to figure out if this is
            // a) already in a loop and we continue on with that loop
            // b) already in a loop and need to break out
            // c) not in a loop and need to start one
            // d) not in a loop and doesn't need to start one

            let this = self.next?;
            match self.iterator.next() {
                Some(i) => self.next = Some(i),
                None => {
                    // invalidate next, so it None will be returned next time
                    self.next = None;

                    // return the one we have
                    return Some(this.as_raw());
                }
            };

            // the list must be sorted
            assert!(this < self.next?, "error: {}!<{}", this.0, self.next?.0);

            if let Some(last_in_loop) = self.last_in_loop {
                if last_in_loop + 2 == self.next? {
                    // continue with the loop
                    self.last_in_loop = Some(last_in_loop + 1);

                    continue;
                } else {
                    // break out of the loop
                    self.last_in_loop = None;

                    return Some(this.as_raw());
                }
            } else if this + 1 == self.next? {
                // create a loop
                self.last_in_loop = Some(this);

                // set the first bit to 1
                return Some(this.as_raw() | 1 << 31);
            } else {
                // no looping necessary
                return Some(this.as_raw());
            }
        }
    }
}

// keep in mind loss_list must be sorted
// takes in a list of u32, which is the loss list
pub fn compress_loss_list<I>(loss_list: I) -> CompressLossList<I>
where
    I: Iterator<Item = SeqNumber>,
{
    CompressLossList {
        iterator: loss_list,
        next: None,
        last_in_loop: None,
    }
}

pub struct DecompressLossList<I> {
    iterator: I,

    loop_next_end: Option<(u32, u32)>,
}

impl<I: Iterator<Item = u32>> Iterator for DecompressLossList<I> {
    type Item = SeqNumber;

    fn next(&mut self) -> Option<SeqNumber> {
        match self.loop_next_end {
            Some((next, end)) if next == end => {
                // loop is over
                self.loop_next_end = None;

                Some(SeqNumber::new_truncate(next))
            }
            Some((next, end)) => {
                // continue the loop
                self.loop_next_end = Some((next + 1, end));

                Some(SeqNumber::new_truncate(next))
            }
            None => {
                // no current loop
                let next = self.iterator.next()?;

                // is this a loop start
                if next & (1 << 31) != 0 {
                    // set the first bit to zero
                    let next_num = next & !(1 << 31);
                    self.loop_next_end = Some((
                        next_num + 1,
                        match self.iterator.next() {
                            Some(i) => i,
                            None => panic!("unterminated loop while decompressing loss list"),
                        },
                    ));

                    Some(SeqNumber::new_truncate(next_num))
                } else {
                    // no looping is possible
                    Some(SeqNumber::new_truncate(next))
                }
            }
        }
    }
}

pub fn decompress_loss_list<I: Iterator<Item = u32>>(loss_list: I) -> DecompressLossList<I> {
    DecompressLossList {
        iterator: loss_list,
        loop_next_end: None,
    }
}

#[cfg(test)]
mod test {

    use super::{compress_loss_list, decompress_loss_list};
    use crate::SeqNumber;

    const ONE: u32 = 1 << 31;

    #[test]
    fn loss_compression_test() {
        macro_rules! test_comp_decomp {
            ($x:expr, $y:expr) => {{
                assert_eq!(
                    compress_loss_list($x.iter().cloned().map(SeqNumber::new_truncate))
                        .collect::<Vec<_>>(),
                    $y.iter().cloned().collect::<Vec<_>>(),
                    "Compressed wasn't same as given"
                );
                assert_eq!(
                    decompress_loss_list($y.iter().cloned()).collect::<Vec<_>>(),
                    $x.iter()
                        .cloned()
                        .map(SeqNumber::new_truncate)
                        .collect::<Vec<_>>(),
                    "Decompressed not same as given"
                );
            }};
        }

        test_comp_decomp!([13, 14, 15, 16, 17, 18, 19], [13 | ONE, 19]);

        test_comp_decomp!(
            [1, 2, 3, 4, 5, 9, 11, 12, 13, 16, 17],
            [1 | ONE, 5, 9, 11 | ONE, 13, 16 | ONE, 17]
        );

        test_comp_decomp!([15, 16], [15 | ONE, 16]);

        test_comp_decomp!(
            [1_687_761_238, 1_687_761_239],
            [1_687_761_238 | ONE, 1_687_761_239]
        );
    }

    #[test]
    #[should_panic(expected = "error: 10!<1")]
    fn invalid_ordering() {
        let _ = compress_loss_list([10, 1].iter().copied().map(SeqNumber::new_truncate))
            .collect::<Vec<_>>();
    }

    #[test]
    #[should_panic(expected = "unterminated loop")]
    fn unterminated_loop() {
        let _ = decompress_loss_list([10 | ONE].iter().copied()).collect::<Vec<_>>();
    }
}
