#pragma once

#include <algorithm>
#include <array>
#include <cmath>
#include <concepts>
#include <cstddef>
#include <cstdint>
#include <initializer_list>
#include <optional>
#include <span>
#include <utility>
#include <kiyosi/core/error.hpp>

namespace kiyosi {

namespace detail {
inline Result<double> checked_price(double value)
{
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "pricing produced no finite price"});
    return value;
}
} // namespace detail

/// Public Greek contract:
/// - delta, gamma, and speed are price changes per one spot unit, squared spot unit, and cubed
///   spot unit, respectively;
/// - vega, vanna, and zomma are price, delta, and gamma changes per one volatility percentage
///   point (an absolute volatility change of 0.01);
/// - rho is the price change per one interest-rate percentage point (an absolute rate change of
///   0.01);
/// - theta, charm, and color are price, delta, and gamma changes per calendar day as valuation
///   time moves forward.
/// Unrequested, undefined, or unsupported Greeks are unavailable (`std::nullopt`), never zero sentinels.
enum class Greek : std::uint8_t {
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

/// Number of defined Greek values.
inline constexpr std::size_t greek_count = static_cast<std::size_t>(Greek::rho) + 1;

/// Converts a Greek to its PricingResult storage index.
/// @return The index, or `std::nullopt` for an unknown enumerator.
[[nodiscard]] constexpr std::optional<std::size_t> greek_index(Greek greek) noexcept
{
    const auto index = static_cast<std::size_t>(greek);
    return index < greek_count ? std::optional{index} : std::nullopt;
}

/// Greeks requested from a joint valuation. Price is always included separately.
/// Construct from one or more Greek values, or from true for every Greek.
/// Duplicates are ignored; empty requests and unknown values are rejected
/// by price_with_greeks() with an invalid_parameter error.
class GreeksRequest {
public:
    /// Used internally for price-only valuations; invalid for price_with_greeks().
    GreeksRequest() = default;
    GreeksRequest(std::initializer_list<Greek> greeks) : GreeksRequest(std::span{greeks.begin(), greeks.size()}) {}
    explicit GreeksRequest(std::span<const Greek> greeks)
    {
        for (const auto greek : greeks) {
            const auto index = greek_index(greek);
            if (!index) invalid_ = true;
            else selected_[*index] = true;
        }
    }
    // `true` is the documented shorthand for requesting every Greek.
    template <std::same_as<bool> T>
    GreeksRequest(T all_greeks) // NOLINT(google-explicit-constructor)
    {
        selected_.fill(all_greeks);
    }

    [[nodiscard]] bool has(Greek greek) const noexcept
    {
        const auto index = greek_index(greek);
        return index && selected_[*index];
    }
    [[nodiscard]] bool empty() const noexcept
    {
        return !std::ranges::contains(selected_, true);
    }
    [[nodiscard]] Result<void> validate() const
    {
        if (invalid_ || empty())
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "invalid Greeks request"});
        return {};
    }

private:
    std::array<bool, greek_count> selected_{};
    bool invalid_ = false;
};

/// Price and a fixed-size collection of optional Greeks.
class PricingResult {
public:
    /// Storage type indexed by greek_index().
    using GreekValues = std::array<std::optional<double>, greek_count>;

    /// Price is present in every successfully constructed result.
    [[nodiscard]] double price() const noexcept { return price_; }

    /// Reports whether the Greek is available; a stored zero is available.
    [[nodiscard]] bool has(Greek greek) const noexcept
    {
        const auto index = greek_index(greek);
        return index && values_[*index].has_value();
    }

    /// Retrieves a Greek when its enumerator is valid.
    /// @return The optional value, or an `invalid_parameter` error for an unknown Greek.
    [[nodiscard]] Result<std::optional<double>> get(Greek greek) const
    {
        const auto index = greek_index(greek);
        if (!index)
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "unknown Greek"});
        return values_[*index];
    }

    /// Retrieves a required Greek.
    /// @return The value, or an `invalid_parameter` or `invalid_result` error.
    [[nodiscard]] Result<double> require(Greek greek) const
    {
        const auto value = get(greek);
        if (!value) return std::unexpected(value.error());
        if (*value) return **value;
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "requested Greek is unavailable"});
    }

    /// Returns a read-only view of all Greek slots.
    /// @note The reference remains valid until this result is destroyed, moved from, or assigned.
    [[nodiscard]] const GreekValues& values_view() const noexcept { return values_; }

    /// Returns price and only the requested Greeks.
    [[nodiscard]] PricingResult selected(GreeksRequest greeks) const noexcept
    {
        PricingResult output = *this;
        for (std::size_t index = 0; index < greek_count; ++index)
            if (!greeks.has(static_cast<Greek>(index))) output.values_[index].reset();
        return output;
    }

    /// Tests whether price and every available Greek are finite.
    [[nodiscard]] bool all_finite() const noexcept
    {
        if (!std::isfinite(price_)) return false;
        for (const auto& value : values_)
            if (value && !std::isfinite(*value)) return false;
        return true;
    }

private:
    friend Result<PricingResult> make_pricing_result(
        double, std::initializer_list<std::pair<Greek, std::optional<double>>>);

    explicit PricingResult(double price) : price_(price) {}

    double price_;
    GreekValues values_{};
};

/// Builds a result from a price and runtime Greek entries.
/// Unknown Greeks are rejected with `invalid_parameter`.
/// Later duplicate entries replace earlier entries for the same Greek.
/// @return The populated result, or an `invalid_parameter` error.
[[nodiscard]] inline Result<PricingResult> make_pricing_result(
    double price, std::initializer_list<std::pair<Greek, std::optional<double>>> entries = {})
{
    PricingResult output{price};
    for (const auto& [greek, value] : entries) {
        const auto index = greek_index(greek);
        if (!index)
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "unknown Greek"});
        output.values_[*index] = value;
    }
    return output;
}

} // namespace kiyosi
