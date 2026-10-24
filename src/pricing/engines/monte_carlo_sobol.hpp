#pragma once

#include <bit>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <vector>

#include "monte_carlo_normal.hpp"

namespace kiyosi::detail {

inline constexpr unsigned int sobol_bits = 52;

// A request owns the immutable scramble; independent cursors may share it concurrently.
class SobolSequence {
public:
    SobolSequence(std::size_t dimensions, std::uint64_t seed);

    [[nodiscard]] std::size_t dimensions() const noexcept { return shifts_.size(); }

private:
    friend class SobolCursor;
    std::vector<std::uint64_t> directions_;
    std::vector<std::uint64_t> shifts_;
};

// The sequence must outlive its cursors; each cursor belongs to one worker.
class SobolCursor {
public:
    explicit SobolCursor(const SobolSequence& sequence, std::uint64_t path_index = 0)
        : sequence_(&sequence), values_(sequence.shifts_), index_(path_index)
    {
        assert(path_index < (std::uint64_t{1} << sobol_bits));
        auto gray = path_index ^ (path_index >> 1U);
        while (gray != 0) {
            const auto column = static_cast<std::size_t>(std::countr_zero(gray));
            for (std::size_t dimension = 0; dimension < values_.size(); ++dimension)
                values_[dimension] ^= sequence_->directions_[column * values_.size() + dimension];
            gray &= gray - 1U;
        }
    }

    [[nodiscard]] double uniform() noexcept
    {
        assert(dimension_ < values_.size());
        // Midpoints retain every point, including index zero, without inverse-CDF infinities.
        return (static_cast<double>(values_[dimension_++]) + 0.5) * 0x1p-52;
    }

    [[nodiscard]] double normal() noexcept { return inverse_normal_cdf(uniform()); }

    void next_path() noexcept
    {
        ++index_;
        assert(index_ < (std::uint64_t{1} << sobol_bits));
        const auto column = static_cast<std::size_t>(std::countr_zero(index_));
        for (std::size_t dimension = 0; dimension < values_.size(); ++dimension)
            values_[dimension] ^= sequence_->directions_[column * values_.size() + dimension];
        dimension_ = 0;
    }

private:
    const SobolSequence* sequence_;
    std::vector<std::uint64_t> values_;
    std::uint64_t index_;
    std::size_t dimension_{};
};

} // namespace kiyosi::detail
