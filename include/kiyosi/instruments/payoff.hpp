#pragma once

#include <cmath>
#include <concepts>
#include <type_traits>
#include <variant>

#include <kiyosi/core/error.hpp>

namespace kiyosi {

/// Requirement for a value type used as an option payoff tag.
template <typename Value>
concept OptionPayoff = std::copy_constructible<std::remove_cvref_t<Value>> &&
                       std::equality_comparable<std::remove_cvref_t<Value>>;

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
enum class PayoffType {
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

} // namespace kiyosi
