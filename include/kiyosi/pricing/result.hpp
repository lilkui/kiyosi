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

/// Explicit work requested by price_with_greeks(); no default tier is implied.
enum class GreeksLevel : std::uint8_t {
    basic, ///< Price, delta, and gamma.
    full,  ///< Price and all ten defined Greeks, where available.
};

namespace detail {
enum class RiskMeasureOutput { price_only, basic, all };
}

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
/// Unrequested, undefined, or unsupported measures are unavailable (`std::nullopt`), never zero sentinels.
enum class RiskMeasure : std::uint8_t {
    price, ///< Instrument value.
    delta, ///< First derivative with respect to spot.
    gamma, ///< Second derivative with respect to spot.
    speed, ///< Third derivative with respect to spot.
    theta, ///< Value change per calendar day of forward valuation time.
    charm, ///< Delta change per calendar day of forward valuation time.
    color, ///< Gamma change per calendar day of forward valuation time.
    vega,  ///< Value change per volatility percentage point.
    vanna, ///< Delta change per volatility percentage point.
    zomma, ///< Gamma change per volatility percentage point.
    rho,   ///< Value change per interest-rate percentage point.
};

/// Number of defined RiskMeasure values.
inline constexpr std::size_t risk_measure_count = static_cast<std::size_t>(RiskMeasure::rho) + 1;

/// Converts a risk measure to its PricingResult storage index.
/// @return The index, or `std::nullopt` for an unknown enumerator.
[[nodiscard]] constexpr std::optional<std::size_t> risk_measure_index(RiskMeasure measure) noexcept
{
    const auto index = static_cast<std::size_t>(measure);
    return index < risk_measure_count ? std::optional{index} : std::nullopt;
}

/// Fixed-size collection of optional pricing and risk measures.
class PricingResult {
public:
    /// Storage type indexed by risk_measure_index().
    using MeasureValues = std::array<std::optional<double>, risk_measure_count>;

    /// Creates a result with every measure unavailable.
    PricingResult() = default;

    /// Reports whether the measure is available; a stored zero is available.
    [[nodiscard]] bool has(RiskMeasure measure) const noexcept
    {
        const auto index = risk_measure_index(measure);
        return index && values_[*index].has_value();
    }

    /// Retrieves a measure when its enumerator is valid.
    /// @return The optional value, or an `invalid_parameter` error for an unknown measure.
    [[nodiscard]] Result<std::optional<double>> get(RiskMeasure measure) const
    {
        const auto index = risk_measure_index(measure);
        if (!index)
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "unknown risk measure"});
        return values_[*index];
    }

    /// Retrieves a required measure.
    /// @return The value, or an `invalid_parameter` or `invalid_result` error.
    [[nodiscard]] Result<double> require(RiskMeasure measure) const
    {
        const auto value = get(measure);
        if (!value) return std::unexpected(value.error());
        if (*value) return **value;
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "requested risk measure is unavailable"});
    }

    /// Returns a read-only view of all measure slots.
    /// @note The reference remains valid until this result is destroyed, moved from, or assigned.
    [[nodiscard]] const MeasureValues& values_view() const noexcept { return values_; }

    /// Tests whether every available measure is finite.
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
/// Later duplicate entries replace earlier entries for the same measure.
/// @return The populated result, or an `invalid_parameter` error.
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
