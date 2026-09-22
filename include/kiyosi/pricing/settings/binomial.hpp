#pragma once

namespace kiyosi {

/// Aggregate configuration validated by binomial engines when price() is called.
struct BinomialSettings {
    int step_count = 256; ///< Number of tree time steps; must be positive.
};

} // namespace kiyosi
