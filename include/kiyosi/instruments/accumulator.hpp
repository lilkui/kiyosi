#pragma once

#include <cmath>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>

namespace kiyosi {

class Accumulator;

struct AccumulatorTerms {
    double strike{};
    double knock_out_level{};
    double daily_quantity{};
    double acceleration_factor{};
    double accumulated_quantity{};
    Date effective_date{};
    Date expiry_date{};
};

[[nodiscard]] Result<Accumulator> make_accumulator(AccumulatorTerms);

/// Forward accrual that buys a fixed daily quantity, accelerating below strike and
/// terminating once spot reaches the knock-out level.
class Accumulator {
public:
    double strike() const noexcept { return strike_; }
    double knock_out_level() const noexcept { return knock_out_level_; }
    double daily_quantity() const noexcept { return daily_quantity_; }
    double acceleration_factor() const noexcept { return acceleration_factor_; }
    double accumulated_quantity() const noexcept { return accumulated_quantity_; }
    Date effective_date() const noexcept { return effective_date_; }
    Date expiry_date() const noexcept { return expiry_date_; }
    friend bool operator==(const Accumulator&, const Accumulator&) = default;

private:
    Accumulator(double strike, double knock_out_level, double daily_quantity, double acceleration_factor,
                double accumulated_quantity, Date effective_date, Date expiry_date)
        : strike_(strike), knock_out_level_(knock_out_level), daily_quantity_(daily_quantity),
          acceleration_factor_(acceleration_factor), accumulated_quantity_(accumulated_quantity),
          effective_date_(effective_date), expiry_date_(expiry_date) {}

    double strike_, knock_out_level_, daily_quantity_, acceleration_factor_, accumulated_quantity_;
    Date effective_date_, expiry_date_;

    friend Result<Accumulator> make_accumulator(AccumulatorTerms);
};

[[nodiscard]] inline Result<Accumulator> make_accumulator(AccumulatorTerms terms)
{
    const auto [strike, knock_out_level, daily_quantity, acceleration_factor, accumulated_quantity, effective_date, expiry_date] = terms;
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(
            Error{ErrorCategory::invalid_strike, "strike must be finite and positive"});
    if (!std::isfinite(knock_out_level) || knock_out_level <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "knock-out level must be finite and positive"});
    if (!std::isfinite(daily_quantity) || daily_quantity < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "daily quantity must be finite and non-negative"});
    if (!std::isfinite(acceleration_factor) || acceleration_factor < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "acceleration factor must be finite and non-negative"});
    if (!std::isfinite(accumulated_quantity) || accumulated_quantity < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "accumulated quantity must be finite and non-negative"});
    if (!is_supported_date(effective_date))
        return std::unexpected(
            Error{ErrorCategory::invalid_date, "effective date must be a valid calendar date"});
    if (!is_supported_date(expiry_date))
        return std::unexpected(
            Error{ErrorCategory::invalid_date, "expiry date must be a valid calendar date"});
    if (effective_date > expiry_date)
        return std::unexpected(Error{ErrorCategory::invalid_time_range,
                                     "expiry date must not precede the effective date"});
    return Accumulator{strike, knock_out_level, daily_quantity, acceleration_factor, accumulated_quantity,
                       effective_date, expiry_date};
}

} // namespace kiyosi
