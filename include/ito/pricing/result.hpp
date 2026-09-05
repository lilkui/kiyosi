#pragma once

#include <optional>
#include <ito/core/types.hpp>

namespace ito {

enum class risk_measure : std::uint16_t {
    price = 1u << 0,
    delta = 1u << 1,
    gamma = 1u << 2,
    speed = 1u << 3,
    theta = 1u << 4,
    charm = 1u << 5,
    color = 1u << 6,
    vega = 1u << 7,
    vanna = 1u << 8,
    zomma = 1u << 9,
    rho = 1u << 10,
};

using risk_measure_set = std::uint16_t;

[[nodiscard]] constexpr risk_measure_set risk_bit(risk_measure measure) noexcept
{
    return static_cast<risk_measure_set>(measure);
}

[[nodiscard]] constexpr risk_measure_set operator|(risk_measure left, risk_measure right) noexcept
{
    return risk_bit(left) | risk_bit(right);
}

[[nodiscard]] constexpr risk_measure_set operator|(risk_measure_set left, risk_measure right) noexcept
{
    return left | risk_bit(right);
}

[[nodiscard]] constexpr risk_measure_set operator|(risk_measure left, risk_measure_set right) noexcept
{
    return risk_bit(left) | right;
}

inline constexpr risk_measure_set all_risk_measures =
    risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma) |
    risk_bit(risk_measure::speed) | risk_bit(risk_measure::theta) | risk_bit(risk_measure::charm) |
    risk_bit(risk_measure::color) | risk_bit(risk_measure::vega) | risk_bit(risk_measure::vanna) |
    risk_bit(risk_measure::zomma) | risk_bit(risk_measure::rho);

struct PricingResult {
    /// Present value in the input asset-price currency units.
    double value;
    /// Change in value per one unit of underlying price.
    double delta;
    /// Change in delta per one unit of underlying price.
    double gamma;
    /// Change in gamma per one unit of underlying price.
    double speed;
    /// Per calendar day under Actual/365 Fixed.
    double theta;
    /// Per calendar day under Actual/365 Fixed.
    double charm;
    /// Per calendar day under Actual/365 Fixed.
    double color;
    /// Per one percentage-point volatility move.
    double vega;
    /// Per one percentage-point volatility move.
    double vanna;
    /// Per one percentage-point volatility move.
    double zomma;
    /// Per one percentage-point rate move.
    double rho;

    risk_measure_set available = all_risk_measures;

    [[nodiscard]] bool has(risk_measure measure) const noexcept;
    [[nodiscard]] std::optional<double> get(risk_measure measure) const noexcept;
};

struct PricingRequest {
    risk_measure_set measures = all_risk_measures;

    [[nodiscard]] static constexpr PricingRequest price_only() noexcept
    {
        return PricingRequest{risk_bit(risk_measure::price)};
    }
    [[nodiscard]] static constexpr PricingRequest all() noexcept { return PricingRequest{}; }
    [[nodiscard]] constexpr bool requests(risk_measure measure) const noexcept
    {
        return (measures & risk_bit(measure)) != 0;
    }
};

[[nodiscard]] inline bool PricingResult::has(risk_measure measure) const noexcept
{
    return (available & risk_bit(measure)) != 0;
}

[[nodiscard]] inline std::optional<double> PricingResult::get(risk_measure measure) const noexcept
{
    if (!has(measure)) return std::nullopt;
    switch (measure) {
    case risk_measure::price:
        return value;
    case risk_measure::delta:
        return delta;
    case risk_measure::gamma:
        return gamma;
    case risk_measure::speed:
        return speed;
    case risk_measure::theta:
        return theta;
    case risk_measure::charm:
        return charm;
    case risk_measure::color:
        return color;
    case risk_measure::vega:
        return vega;
    case risk_measure::vanna:
        return vanna;
    case risk_measure::zomma:
        return zomma;
    case risk_measure::rho:
        return rho;
    }
    return std::nullopt;
}

} // namespace ito
