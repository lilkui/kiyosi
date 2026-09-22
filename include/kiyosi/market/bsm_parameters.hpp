#pragma once

#include <cmath>

#include <kiyosi/core/error.hpp>

namespace kiyosi {

/// Black-Scholes-Merton model parameters: continuously compounded rates and a flat volatility.
class BlackScholesMertonParameters {
public:
    /// Returns the continuously compounded risk-free rate as a decimal.
    double risk_free_rate() const noexcept { return risk_free_rate_; }
    /// Returns the continuously compounded dividend yield as a decimal.
    double dividend_yield() const noexcept { return dividend_yield_; }
    /// Returns the positive annualized volatility as a decimal.
    double volatility() const noexcept { return volatility_; }

    /// Compares all model parameters.
    friend bool operator==(const BlackScholesMertonParameters&, const BlackScholesMertonParameters&) = default;

private:
    BlackScholesMertonParameters(double risk_free_rate, double dividend_yield, double volatility)
        : risk_free_rate_(risk_free_rate), dividend_yield_(dividend_yield), volatility_(volatility) {}

    double risk_free_rate_;
    double dividend_yield_;
    double volatility_;

    friend Result<BlackScholesMertonParameters> make_bsm_parameters(double, double, double);
};

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
    return BlackScholesMertonParameters{risk_free_rate, dividend_yield, volatility};
}

} // namespace kiyosi
