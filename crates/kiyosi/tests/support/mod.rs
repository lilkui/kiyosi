use kiyosi::*;

pub trait DatedHistory: OptionContract + Clone {
    fn at_snapshot(&self, at: Timestamp) -> Result<Self>;
}

macro_rules! history {
    ($($name:ty => $field:ident),* $(,)?) => {$(
        impl DatedHistory for $name {
            fn at_snapshot(&self, at: Timestamp) -> Result<Self> {
                self.replace_history(self.$field(), Some(at))
            }
        }
    )*};
}
history!(
    BarrierOption => touch_state,
    DoubleBarrierOption => touch_state,
    BinaryBarrierOption => touch_state,
    TouchOption => touch_state,
    DoubleTouchOption => touch_state,
    RangeAccrualOption => realized_hit_count,
);
macro_rules! average_history {
    ($($name:ty),* $(,)?) => {$(
        impl DatedHistory for $name {
            fn at_snapshot(&self, at: Timestamp) -> Result<Self> {
                if self.realized_average().is_some() && at <= self.averaging_start_time() {
                    Ok(self.clone())
                } else {
                    self.replace_history(self.realized_average(), Some(at))
                }
            }
        }
    )*};
}
average_history!(GeometricAveragePriceOption, ArithmeticAveragePriceOption);
impl<T: AutocallableTerms> DatedHistory for AutocallableNote<T> {
    fn at_snapshot(&self, at: Timestamp) -> Result<Self> {
        self.replace_history(self.barrier_state(), Some(at))
    }
}
impl DatedHistory for Accumulator {
    fn at_snapshot(&self, at: Timestamp) -> Result<Self> {
        self.replace_history(
            AccumulatorHistoryChanges {
                accumulated_quantity: Some(self.accumulated_quantity()),
                knock_out_settled: Some(self.knock_out_settled()),
            },
            Some(at),
        )
    }
}
macro_rules! history_free {
    ($($name:ty),* $(,)?) => {$(
        impl DatedHistory for $name {
            fn at_snapshot(&self, _: Timestamp) -> Result<Self> { Ok(self.clone()) }
        }
    )*};
}
history_free!(EuropeanOption, AmericanOption, DigitalOption);

pub fn dated<I: DatedHistory>(instrument: &I, context: &PricingContext) -> I {
    if context.valuation_time() >= instrument.effective_time()
        && context.valuation_time() <= instrument.expiry_time()
    {
        instrument
            .at_snapshot(context.valuation_time())
            .expect("valid test history snapshot")
    } else {
        instrument.clone()
    }
}
