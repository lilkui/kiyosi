#pragma once

#include <cmath>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>

namespace kiyosi {

class Accumulator;

struct AccumulatorTerms {
    double strike{};
    double knock_out{};
    double daily_quantity{};
    double acceleration{};
    double accumulated_quantity{};
    date effective{};
    date expiry{};
};

[[nodiscard]] result<Accumulator> make_accumulator(AccumulatorTerms);

/// Forward accrual that buys a fixed daily quantity, accelerating below strike and
/// terminating once spot reaches the knock-out level.
class Accumulator {
public:
    double strike() const noexcept { return strike_; }
    double knock_out() const noexcept { return knock_out_; }
    double daily_quantity() const noexcept { return daily_quantity_; }
    double acceleration() const noexcept { return acceleration_; }
    double accumulated_quantity() const noexcept { return accumulated_quantity_; }
    date effective() const noexcept { return effective_; }
    date expiry() const noexcept { return expiry_; }
    friend bool operator==(const Accumulator&, const Accumulator&) = default;

private:
    Accumulator(double strike, double knock_out, double daily_quantity, double acceleration,
                double accumulated_quantity, date effective, date expiry)
        : strike_(strike), knock_out_(knock_out), daily_quantity_(daily_quantity),
          acceleration_(acceleration), accumulated_quantity_(accumulated_quantity),
          effective_(effective), expiry_(expiry) {}

    double strike_, knock_out_, daily_quantity_, acceleration_, accumulated_quantity_;
    date effective_, expiry_;

    friend result<Accumulator> make_accumulator(AccumulatorTerms);
};

[[nodiscard]] inline result<Accumulator> make_accumulator(AccumulatorTerms terms)
{
    const auto [strike, knock_out, daily_quantity, acceleration, accumulated_quantity, effective, expiry] = terms;
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(
            Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!std::isfinite(knock_out) || knock_out <= 0.0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "knock-out price must be finite and positive"});
    if (!std::isfinite(daily_quantity) || daily_quantity < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "daily quantity must be finite and non-negative"});
    if (!std::isfinite(acceleration) || acceleration < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "acceleration must be finite and non-negative"});
    if (!std::isfinite(accumulated_quantity) || accumulated_quantity < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "accumulated quantity must be finite and non-negative"});
    if (!is_valid_date(effective))
        return std::unexpected(
            Error{error_category::invalid_date, "effective must be a valid calendar date"});
    if (!is_valid_date(expiry))
        return std::unexpected(
            Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    if (effective > expiry)
        return std::unexpected(Error{error_category::invalid_expiry,
                                     "expiry must not precede effective"});
    return Accumulator{strike, knock_out, daily_quantity, acceleration, accumulated_quantity,
                       effective, expiry};
}

} // namespace kiyosi
