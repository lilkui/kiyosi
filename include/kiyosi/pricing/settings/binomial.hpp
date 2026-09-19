#pragma once

namespace kiyosi {

/// Aggregate configuration validated by binomial engines when price() is called.
struct BinomialSettings {
    int step_count = 256;
};

} // namespace kiyosi
