#pragma once

#include <cmath>
#include <optional>

#include <kiyosi/core/error.hpp>
#include <kiyosi/market/rate_term_structure.hpp>

namespace kiyosi {

/// Fields to replace; absent fields retain their current values.
struct BlackScholesMertonParameterChanges {
    std::optional<double> risk_free_rate{};
    std::optional<double> dividend_yield{};
    std::optional<double> volatility{};
    std::optional<RateTermStructure> risk_free_curve{};
    std::optional<RateTermStructure> dividend_curve{};
};

/// Black-Scholes-Merton model parameters: continuously compounded rates and a flat volatility.
class BlackScholesMertonParameters {
public:
    /// Returns a validated owning copy with the supplied fields replaced.
    /// Uses make_bsm_parameters validation and error categories; never changes this object.
    /// Concurrent calls and reads are safe without concurrent assignment to this object.
    [[nodiscard]] Result<BlackScholesMertonParameters> replace(BlackScholesMertonParameterChanges changes = {}) const;

    /// Returns the flat rate or unsupported_operation for a node-based curve.
    [[nodiscard]] Result<double> risk_free_rate() const
    {
        if (const auto rate = risk_free_curve_.flat_rate()) return *rate;
        return std::unexpected(Error{ErrorCategory::unsupported_operation, "risk-free curve is not explicitly flat"});
    }
    /// Returns the flat yield or unsupported_operation for a node-based curve.
    [[nodiscard]] Result<double> dividend_yield() const
    {
        if (const auto rate = dividend_curve_.flat_rate()) return *rate;
        return std::unexpected(Error{ErrorCategory::unsupported_operation, "dividend curve is not explicitly flat"});
    }
    const RateTermStructure& risk_free_curve() const noexcept { return risk_free_curve_; }
    const RateTermStructure& dividend_curve() const noexcept { return dividend_curve_; }
    bool has_flat_rates() const noexcept { return risk_free_curve_.is_flat() && dividend_curve_.is_flat(); }
    /// Returns the positive annualized volatility as a decimal.
    double volatility() const noexcept { return volatility_; }

    /// Compares all model parameters.
    friend bool operator==(const BlackScholesMertonParameters&, const BlackScholesMertonParameters&) = default;

private:
    BlackScholesMertonParameters(RateTermStructure risk_free_curve, RateTermStructure dividend_curve, double volatility)
        : risk_free_curve_(std::move(risk_free_curve)), dividend_curve_(std::move(dividend_curve)), volatility_(volatility) {}

    RateTermStructure risk_free_curve_;
    RateTermStructure dividend_curve_;
    double volatility_;

    friend Result<BlackScholesMertonParameters> make_bsm_parameters(double, double, double);
    friend Result<BlackScholesMertonParameters> make_bsm_parameters(RateTermStructure, RateTermStructure, double);
};

/// Creates model parameters from validated owning curves and a positive flat volatility.
[[nodiscard]] inline Result<BlackScholesMertonParameters> make_bsm_parameters(
    RateTermStructure risk_free_curve, RateTermStructure dividend_curve, double volatility)
{
    if (!std::isfinite(volatility) || volatility <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_volatility, "volatility must be finite and positive"});
    return BlackScholesMertonParameters{std::move(risk_free_curve), std::move(dividend_curve), volatility};
}

/// Creates validated Black-Scholes-Merton model parameters.
/// @return The parameters, or a rate, yield, or volatility validation error.
[[nodiscard]] inline Result<BlackScholesMertonParameters> make_bsm_parameters(
    double risk_free_rate, double dividend_yield, double volatility)
{
    if (!std::isfinite(risk_free_rate))
        return std::unexpected(Error{ErrorCategory::invalid_risk_free_rate, "risk-free rate must be finite"});
    if (!std::isfinite(dividend_yield))
        return std::unexpected(Error{ErrorCategory::invalid_dividend_yield, "dividend yield must be finite"});
    if (!std::isfinite(volatility) || volatility <= 0.0) {
        return std::unexpected(Error{ErrorCategory::invalid_volatility,
                                     "volatility must be finite and positive"});
    }
    return make_bsm_parameters(*make_flat_rate(risk_free_rate), *make_flat_rate(dividend_yield), volatility);
}

inline Result<BlackScholesMertonParameters> BlackScholesMertonParameters::replace(
    BlackScholesMertonParameterChanges changes) const
{
    if ((changes.risk_free_rate && changes.risk_free_curve) || (changes.dividend_yield && changes.dividend_curve))
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "replace either a scalar rate or its curve, not both"});
    if (changes.risk_free_rate && !std::isfinite(*changes.risk_free_rate))
        return std::unexpected(Error{ErrorCategory::invalid_risk_free_rate, "risk-free rate must be finite"});
    if (changes.dividend_yield && !std::isfinite(*changes.dividend_yield))
        return std::unexpected(Error{ErrorCategory::invalid_dividend_yield, "dividend yield must be finite"});
    return make_bsm_parameters(
        changes.risk_free_rate ? *make_flat_rate(*changes.risk_free_rate) : changes.risk_free_curve.value_or(risk_free_curve_),
        changes.dividend_yield ? *make_flat_rate(*changes.dividend_yield) : changes.dividend_curve.value_or(dividend_curve_),
        changes.volatility.value_or(volatility_));
}

/// Creates parameters with a risk-free curve and scalar flat dividend yield.
[[nodiscard]] inline Result<BlackScholesMertonParameters> make_bsm_parameters(
    RateTermStructure risk_free_curve, double dividend_yield, double volatility)
{
    if (!std::isfinite(dividend_yield))
        return std::unexpected(Error{ErrorCategory::invalid_dividend_yield, "dividend yield must be finite"});
    return make_bsm_parameters(std::move(risk_free_curve), *make_flat_rate(dividend_yield), volatility);
}

/// Creates parameters with a scalar flat risk-free rate and dividend yield curve.
[[nodiscard]] inline Result<BlackScholesMertonParameters> make_bsm_parameters(
    double risk_free_rate, RateTermStructure dividend_curve, double volatility)
{
    if (!std::isfinite(risk_free_rate))
        return std::unexpected(Error{ErrorCategory::invalid_risk_free_rate, "risk-free rate must be finite"});
    return make_bsm_parameters(*make_flat_rate(risk_free_rate), std::move(dividend_curve), volatility);
}

} // namespace kiyosi
