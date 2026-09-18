#pragma once

namespace kiyosi {

/// Aggregate configuration validated by binomial engines when price() is called.
struct BinomialSettings {
    int steps = 256;
};

} // namespace kiyosi
