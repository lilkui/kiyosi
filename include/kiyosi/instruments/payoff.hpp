#pragma once

#include <cstdint>

#include <cmath>
#include <concepts>
#include <type_traits>
#include <variant>

#include <kiyosi/core/error.hpp>
#include <kiyosi/instruments/option_terms.hpp>

namespace kiyosi {

/// Tag for a standard call or put payoff.
struct VanillaPayoff {
    /// All vanilla-payoff tags compare equal.
    friend bool operator==(const VanillaPayoff&, const VanillaPayoff&) = default;
};

/// Tag for a binary payoff equal to the underlying asset value.
struct AssetOrNothingPayoff {
    /// All asset-or-nothing payoff tags compare equal.
    friend bool operator==(const AssetOrNothingPayoff&, const AssetOrNothingPayoff&) = default;
};

/// Binary payoff denomination.
enum class PayoffType : std::uint8_t {
    cash, ///< Fixed cash payout.
    asset ///< Underlying-asset payout.
};

class CashOrNothingPayoff;

namespace detail {
[[nodiscard]] Result<CashOrNothingPayoff> make_cash_or_nothing_payoff(double);
}

/// Validated fixed-cash binary payoff.
class CashOrNothingPayoff {
public:
    /// Returns the positive cash payout.
    double payout() const noexcept { return payout_; }
    /// Compares payout amounts.
    friend bool operator==(const CashOrNothingPayoff&, const CashOrNothingPayoff&) = default;

private:
    explicit CashOrNothingPayoff(double payout) : payout_(payout) {}
    double payout_;
    friend Result<CashOrNothingPayoff> detail::make_cash_or_nothing_payoff(double);
};

/// Payoff tags whose semantics are defined by the core.
template <typename Value>
concept OptionPayoff = std::same_as<std::remove_cvref_t<Value>, VanillaPayoff> ||
                       std::same_as<std::remove_cvref_t<Value>, CashOrNothingPayoff> ||
                       std::same_as<std::remove_cvref_t<Value>, AssetOrNothingPayoff>;

[[nodiscard]] inline Result<CashOrNothingPayoff> detail::make_cash_or_nothing_payoff(double payout)
{
    if (!std::isfinite(payout) || payout <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "payout must be finite and positive"});
    return CashOrNothingPayoff{payout};
}

/// Cash-or-nothing or asset-or-nothing payoff.
using BinaryPayoff = std::variant<CashOrNothingPayoff, AssetOrNothingPayoff>;

/// Identifies the denomination of a binary payoff.
/// @param payoff Payoff to inspect.
/// @return `PayoffType::cash` or `PayoffType::asset`.
[[nodiscard]] inline PayoffType payoff_type(const BinaryPayoff& payoff) noexcept
{
    return std::holds_alternative<CashOrNothingPayoff>(payoff) ? PayoffType::cash
                                                               : PayoffType::asset;
}

namespace detail {

// Requires validated option direction, strike, spot and payoff terms.
[[nodiscard]] inline double binary_payoff_value(OptionType option_type, double strike, double payout, bool asset, double spot) noexcept
{
    const bool in_the_money = option_type == OptionType::call ? spot > strike : spot < strike;
    return in_the_money ? (asset ? spot : payout) : 0.0;
}

} // namespace detail

} // namespace kiyosi
