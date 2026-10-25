#pragma once

#include <cmath>
#include <optional>

#include <kiyosi/core/time.hpp>

namespace kiyosi {

/// Inputs for an equally weighted, single-asset range coupon; all dates mean 00:00 UTC.
struct RangeAccrualTerms {
    double notional{};
    double annual_coupon_rate{};
    double lower_bound{};
    double upper_bound{};
    Date effective_date{}, expiry_date{};
    std::optional<int> realized_hit_count{}; ///< Hits strictly before valuation; required once prior observations exist.
    friend bool operator==(const RangeAccrualTerms&, const RangeAccrualTerms&) = default;
};

class RangeAccrualOption;
[[nodiscard]] Result<RangeAccrualOption> make_range_accrual_option(RangeAccrualTerms terms);

/// Immutable owning range accrual coupon, without principal repayment or early termination.
/// Observes the context calendar's trading dates in (effective_date, expiry_date], inclusively
/// between fixed price bounds. Pays N*c*T*K/M at expiry, with full-contract ACT/365 Fixed T and M.
/// Calendar, observation count and history/time consistency are validated during pricing.
/// Concurrent const calls and calls on distinct values are safe; assignment requires exclusion.
class RangeAccrualOption {
public:
    double notional() const noexcept { return terms_.notional; }
    double annual_coupon_rate() const noexcept { return terms_.annual_coupon_rate; }
    double lower_bound() const noexcept { return terms_.lower_bound; }
    double upper_bound() const noexcept { return terms_.upper_bound; }
    Date effective_date() const noexcept { return terms_.effective_date; }
    Date expiry_date() const noexcept { return terms_.expiry_date; }
    std::optional<int> realized_hit_count() const noexcept { return terms_.realized_hit_count; }

    /// Returns a validated owning copy; neither success nor failure changes this value.
    [[nodiscard]] Result<RangeAccrualOption> replace(std::optional<int> realized_hit_count) const
    {
        auto terms = terms_;
        terms.realized_hit_count = realized_hit_count;
        return make_range_accrual_option(terms);
    }
    friend bool operator==(const RangeAccrualOption&, const RangeAccrualOption&) = default;

private:
    explicit RangeAccrualOption(RangeAccrualTerms terms) : terms_(terms) {}
    RangeAccrualTerms terms_;
    friend Result<RangeAccrualOption> make_range_accrual_option(RangeAccrualTerms);
};

[[nodiscard]] inline Result<RangeAccrualOption> make_range_accrual_option(RangeAccrualTerms terms)
{
    if (!std::isfinite(terms.notional) || terms.notional <= 0.0 ||
        !std::isfinite(terms.annual_coupon_rate) || terms.annual_coupon_rate < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "notional must be positive and coupon non-negative, both finite"});
    if (!std::isfinite(terms.lower_bound) || !std::isfinite(terms.upper_bound) ||
        terms.lower_bound < 0.0 || terms.lower_bound >= terms.upper_bound)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "range bounds must be finite with 0 <= lower < upper"});
    if (terms.realized_hit_count && *terms.realized_hit_count < 0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "realized hit count must be non-negative"});
    const auto life = validate_instrument_life(terms.effective_date, terms.expiry_date);
    if (!life) return std::unexpected(life.error());
    if (terms.effective_date == terms.expiry_date)
        return std::unexpected(Error{ErrorCategory::invalid_time_range, "range accrual requires a positive contract term"});
    return RangeAccrualOption{terms};
}

} // namespace kiyosi
