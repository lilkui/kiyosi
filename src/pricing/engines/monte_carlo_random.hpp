#pragma once

#include <bit>
#include <cstdint>
#include <limits>

namespace kiyosi::detail {

// PCG-XSH-RR, derived from pcg-cpp (https://www.pcg-random.org); MIT notice in LICENSE.txt.
class Pcg32 {
public:
    using result_type = std::uint32_t;

    Pcg32(std::uint64_t seed, std::uint64_t stream) noexcept
        : increment_((stream << 1U) | 1U)
    {
        (*this)();
        state_ += seed;
        (*this)();
    }

    static constexpr result_type min() noexcept { return 0; }
    static constexpr result_type max() noexcept { return std::numeric_limits<result_type>::max(); }

    result_type operator()() noexcept
    {
        const auto previous = state_;
        state_ = previous * 6364136223846793005ULL + increment_;
        const auto shifted = static_cast<result_type>(((previous >> 18U) ^ previous) >> 27U);
        return std::rotr(shifted, static_cast<int>(previous >> 59U));
    }

private:
    std::uint64_t state_ = 0;
    std::uint64_t increment_;
};

} // namespace kiyosi::detail
