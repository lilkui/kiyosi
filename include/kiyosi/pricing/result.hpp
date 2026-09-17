#pragma once

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <initializer_list>
#include <optional>
#include <stdexcept>
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
enum class risk_measure : std::uint8_t {
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

inline constexpr std::size_t risk_measure_count = static_cast<std::size_t>(risk_measure::rho) + 1;

[[nodiscard]] constexpr std::optional<std::size_t> risk_measure_index(risk_measure measure) noexcept
{
    const auto index = static_cast<std::size_t>(measure);
    return index < risk_measure_count ? std::optional{index} : std::nullopt;
}

class PricingResult {
public:
    using values_type = std::array<std::optional<double>, risk_measure_count>;

    PricingResult() = default;
    PricingResult(std::initializer_list<std::pair<risk_measure, std::optional<double>>> entries)
    {
        for (const auto& [measure, value] : entries) {
            const auto index = risk_measure_index(measure);
            if (!index) throw std::invalid_argument{"unknown risk measure"};
            values_[*index] = value;
        }
    }

    /// Reports whether the measure is available; a stored zero is available.
    [[nodiscard]] bool has(risk_measure measure) const noexcept
    {
        const auto index = risk_measure_index(measure);
        return index && values_[*index].has_value();
    }

    [[nodiscard]] result<std::optional<double>> get(risk_measure measure) const noexcept
    {
        const auto index = risk_measure_index(measure);
        if (!index)
            return std::unexpected(Error{error_category::invalid_parameter,
                                         "unknown risk measure"});
        return values_[*index];
    }

    [[nodiscard]] result<double> require(risk_measure measure) const
    {
        const auto value = get(measure);
        if (!value) return std::unexpected(value.error());
        if (*value) return **value;
        return std::unexpected(Error{error_category::invalid_result,
                                     "requested risk measure is unavailable"});
    }

    [[nodiscard]] const values_type& values_view() const noexcept { return values_; }

    [[nodiscard]] bool all_finite() const noexcept
    {
        for (const auto& value : values_)
            if (value && !std::isfinite(*value)) return false;
        return true;
    }

private:
    values_type values_{};
};

} // namespace kiyosi
