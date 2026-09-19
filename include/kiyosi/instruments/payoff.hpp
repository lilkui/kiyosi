#pragma once

#include <cmath>
#include <concepts>
#include <type_traits>
#include <variant>

#include <kiyosi/core/error.hpp>

namespace kiyosi {

template <typename Value>
concept OptionPayoff = std::copy_constructible<std::remove_cvref_t<Value>> &&
                       std::equality_comparable<std::remove_cvref_t<Value>>;

struct VanillaPayoff {
    friend bool operator==(const VanillaPayoff&, const VanillaPayoff&) = default;
};

struct AssetOrNothingPayoff {
    friend bool operator==(const AssetOrNothingPayoff&, const AssetOrNothingPayoff&) = default;
};

enum class PayoffType { cash,
                         asset };

class CashOrNothingPayoff;

namespace detail {
[[nodiscard]] Result<CashOrNothingPayoff> make_cash_or_nothing_payoff(double);
}

class CashOrNothingPayoff {
public:
    double payout() const noexcept { return payout_; }
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

using BinaryPayoff = std::variant<CashOrNothingPayoff, AssetOrNothingPayoff>;

[[nodiscard]] inline PayoffType payoff_type(const BinaryPayoff& payoff) noexcept
{
    return std::holds_alternative<CashOrNothingPayoff>(payoff) ? PayoffType::cash
                                                               : PayoffType::asset;
}

} // namespace kiyosi
