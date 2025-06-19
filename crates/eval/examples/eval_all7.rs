// code copied from https://github.com/vincev/freezeout

// ```bash
// $ cargo r --release --features=eval --example eval_all7
// ```
use std::time::Instant;

use poker_eval::{Deck, HandRank, HandValue};

fn main() {
    let now = Instant::now();
    let mut agg = [0usize; 9];

    // Evaluate all 133M hands.
    Deck::default().for_each(7, |hand| {
        let rank = HandValue::eval(hand).rank();
        agg[rank as usize] += 1;
    });

    let elapsed = now.elapsed().as_secs_f64();
    let total = agg.iter().sum::<usize>();
    println!("Total hands      {total}");
    println!("Elapsed:         {elapsed:.3}s");
    println!("Hands/sec:       {:.0}\n", total as f64 / elapsed);

    println!("High Card:       {}", agg[HandRank::HighCard as usize]);
    println!("One  Pair:       {}", agg[HandRank::OnePair as usize]);
    println!("Two Pairs:       {}", agg[HandRank::TwoPair as usize]);
    println!("Three of a Kind: {}", agg[HandRank::ThreeOfAKind as usize]);
    println!("Staight:         {}", agg[HandRank::Straight as usize]);
    println!("Flush:           {}", agg[HandRank::Flush as usize]);
    println!("Full House:      {}", agg[HandRank::FullHouse as usize]);
    println!("Four of a Kind:  {}", agg[HandRank::FourOfAKind as usize]);
    println!("Straight Flush:  {}", agg[HandRank::StraightFlush as usize]);
}
