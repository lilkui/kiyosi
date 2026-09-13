#pragma once

#include <cmath>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>

namespace kiyosi {

class Accumulator;

[[nodiscard]] result<Accumulator> make_accumulator(double, double, double, double, double, date, date);

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

    friend result<Accumulator> make_accumulator(double, double, double, double, double, date, date);
};

[[nodiscard]] inline result<Accumulator> make_accumulator(
    double strike, double knock_out, double daily_quantity, double acceleration,
    double accumulated_quantity, date effective, date expiry)
{
    if (!std::isfinite(strike) || strike <= 0.0 || !std::isfinite(knock_out) || knock_out <= 0.0 ||
        !std::isfinite(daily_quantity) || daily_quantity < 0.0 || !std::isfinite(acceleration) ||
        acceleration < 0.0 || !std::isfinite(accumulated_quantity) || accumulated_quantity < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter, "accumulator terms are invalid"});
    if (!is_valid_date(effective) || !is_valid_date(expiry) || effective > expiry)
        return std::unexpected(Error{error_category::invalid_schedule, "accumulator dates are invalid"});
    return Accumulator{strike, knock_out, daily_quantity, acceleration, accumulated_quantity,
                       effective, expiry};
}

} // namespace kiyosi
