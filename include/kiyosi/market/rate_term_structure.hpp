#pragma once

#include <memory>
#include <optional>
#include <vector>

#include <kiyosi/core/day_count.hpp>

namespace kiyosi {
namespace detail {
struct RateCurveNodes;
struct RateTermStructureAccess;
} // namespace detail

/// Immutable, owning continuous-rate term structure. Concurrent reads are safe;
/// assignment or destruction requires external synchronization with readers.
/// Dated curves use Actual/365 Fixed and reject extrapolation. Scalar flat rates
/// have no reference timestamp and cover the supported timestamp range.
class KIYOSI_EXPORT RateTermStructure {
public:
    [[nodiscard]] bool is_flat() const noexcept { return flat_rate_.has_value(); }
    [[nodiscard]] std::optional<double> flat_rate() const noexcept { return flat_rate_; }
    [[nodiscard]] std::optional<Timestamp> reference_time() const noexcept { return reference_time_; }
    [[nodiscard]] std::optional<Timestamp> max_time() const noexcept;
    /// Returns owning copies of the supplied nodes and factors, excluding the implicit reference node.
    [[nodiscard]] std::vector<Timestamp> nodes() const;
    [[nodiscard]] std::vector<double> factors() const;
    /// Validates a timestamp against this curve's coverage.
    [[nodiscard]] Result<void> validate_time(Timestamp time) const;
    /// Returns the log factor over an ordered, covered interval.
    [[nodiscard]] Result<double> log_factor(Timestamp start, Timestamp end) const;
    /// Returns exp(log_factor(start, end)); underflow to zero is allowed.
    [[nodiscard]] Result<double> factor(Timestamp start, Timestamp end) const;
    /// Returns the right-hand instantaneous forward rate; the final node uses its left interval.
    [[nodiscard]] Result<double> forward_rate(Timestamp time) const;
    /// Adds a parallel continuously compounded rate shift without changing reference or coverage.
    [[nodiscard]] Result<RateTermStructure> shifted(double shift) const;
    friend KIYOSI_EXPORT bool operator==(const RateTermStructure&, const RateTermStructure&);

private:
    RateTermStructure(std::optional<Timestamp> reference, std::optional<double> rate,
                      std::shared_ptr<const detail::RateCurveNodes> data = {})
        : reference_time_(reference), flat_rate_(rate), data_(std::move(data)) {}
    [[nodiscard]] double log_factor_unchecked(Timestamp origin, double start, double end) const noexcept;
    [[nodiscard]] double forward_rate_unchecked(Timestamp origin, double time) const noexcept;
    std::optional<Timestamp> reference_time_;
    std::optional<double> flat_rate_;
    std::shared_ptr<const detail::RateCurveNodes> data_;
    double spread_ = 0.0;
    friend Result<RateTermStructure> make_flat_rate(double, std::optional<Timestamp>);
    friend Result<RateTermStructure> make_rate_term_structure(Timestamp, std::vector<Timestamp>, std::vector<double>);
    friend struct detail::RateTermStructureAccess;
};

/// Creates a finite flat annual rate. A reference restricts coverage to timestamps at or after it.
[[nodiscard]] KIYOSI_EXPORT Result<RateTermStructure> make_flat_rate(
    double rate, std::optional<Timestamp> reference_time = {});
/// Creates log-linearly interpolated factors with an implicit factor of one at the reference.
/// Nodes must be strictly increasing after the reference; factors must be finite and positive.
/// Malformed inputs return invalid_parameter, unsupported dates invalid_date.
[[nodiscard]] KIYOSI_EXPORT Result<RateTermStructure> make_rate_term_structure(
    Timestamp reference_time, std::vector<Timestamp> times, std::vector<double> factors);

namespace detail {
/// Pricing access after coverage validation; year offsets avoid rounding solver times to timestamps.
struct RateTermStructureAccess {
    /// Validates coverage and distinct solver-time representations after valuation validation.
    [[nodiscard]] static KIYOSI_EXPORT Result<void> validate_horizon(
        const RateTermStructure& curve, Timestamp valuation, Timestamp end);
    static double log_factor(const RateTermStructure& curve, Timestamp origin, double start, double end) noexcept
    {
        if (curve.flat_rate_) return -*curve.flat_rate_ * (end - start);
        return curve.log_factor_unchecked(origin, start, end);
    }
    static double forward_rate(const RateTermStructure& curve, Timestamp origin, double time) noexcept
    {
        if (curve.flat_rate_) return *curve.flat_rate_;
        return curve.forward_rate_unchecked(origin, time);
    }
};
} // namespace detail
} // namespace kiyosi
