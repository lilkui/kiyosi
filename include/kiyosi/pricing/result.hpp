#pragma once

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <initializer_list>
#include <optional>
#include <utility>
#include <kiyosi/core/error.hpp>

namespace kiyosi {

/// Public risk-measure contract:
/// - price uses the instrument's value units;
/// - delta, gamma, and speed are price changes per one spot unit, squared spot unit, and cubed
///   spot unit, respectively;
/// - vega, vanna, and zomma are price, delta, and gamma changes per one volatility percentage
///   point (an absolute volatility change of 0.01);
/// - rho is the price change per one interest-rate percentage point (an absolute rate change of
///   0.01);
/// - theta, charm, and color are price, delta, and gamma changes per calendar day as valuation
///   time moves forward.
/// Undefined or unsupported measures are unavailable (`std::nullopt`), never represented by zero.
enum class RiskMeasure : std::uint8_t {
    price,
    delta,
    gamma,
    speed,
    theta,
    charm,
    color,
    vega,
    vanna,
    zomma,
    rho,
};

inline constexpr std::size_t risk_measure_count = static_cast<std::size_t>(RiskMeasure::rho) + 1;

[[nodiscard]] constexpr std::optional<std::size_t> risk_measure_index(RiskMeasure measure) noexcept
{
    const auto index = static_cast<std::size_t>(measure);
    return index < risk_measure_count ? std::optional{index} : std::nullopt;
}

class PricingResult {
public:
    using MeasureValues = std::array<std::optional<double>, risk_measure_count>;

    PricingResult() = default;

    /// Reports whether the measure is available; a stored zero is available.
    [[nodiscard]] bool has(RiskMeasure measure) const noexcept
    {
        const auto index = risk_measure_index(measure);
        return index && values_[*index].has_value();
    }

    [[nodiscard]] Result<std::optional<double>> get(RiskMeasure measure) const
    {
        const auto index = risk_measure_index(measure);
        if (!index)
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "unknown risk measure"});
        return values_[*index];
    }

    [[nodiscard]] Result<double> require(RiskMeasure measure) const
    {
        const auto value = get(measure);
        if (!value) return std::unexpected(value.error());
        if (*value) return **value;
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "requested risk measure is unavailable"});
    }

    [[nodiscard]] const MeasureValues& values_view() const noexcept { return values_; }

    [[nodiscard]] bool all_finite() const noexcept
    {
        for (const auto& value : values_)
            if (value && !std::isfinite(*value)) return false;
        return true;
    }

private:
    friend Result<PricingResult> make_pricing_result(
        std::initializer_list<std::pair<RiskMeasure, std::optional<double>>>);

    MeasureValues values_{};
};

/// Builds a result from runtime risk-measure entries.
/// Unknown measures are rejected with `invalid_parameter`.
[[nodiscard]] inline Result<PricingResult> make_pricing_result(
    std::initializer_list<std::pair<RiskMeasure, std::optional<double>>> entries)
{
    PricingResult output;
    for (const auto& [measure, value] : entries) {
        const auto index = risk_measure_index(measure);
        if (!index)
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "unknown risk measure"});
        output.values_[*index] = value;
    }
    return output;
}

} // namespace kiyosi
