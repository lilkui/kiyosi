#pragma once

#include <expected>
#include <string>

namespace kiyosi {

/// Stable categories for errors returned by the public API.
enum class ErrorCategory : unsigned char {
    invalid_option = 1,          ///< An option type or combination is unsupported.
    invalid_strike = 2,          ///< A strike is non-finite or outside its valid range.
    invalid_volatility = 3,      ///< A volatility is non-finite or non-positive.
    invalid_risk_free_rate = 4,  ///< A risk-free rate is non-finite.
    invalid_dividend_yield = 5,  ///< A dividend yield is non-finite.
    invalid_spot_price = 6,      ///< A spot price is non-finite or non-positive.
    invalid_date = 7,            ///< A date is unsupported or invalid for the operation.
    invalid_time_range = 8,      ///< A time range is reversed or outside an instrument life.
    invalid_result = 9,          ///< A requested pricing result is unavailable or invalid.
    invalid_schedule = 10,       ///< Schedule dates or counts violate contract rules.
    invalid_calendar = 11,       ///< A trading-calendar definition is invalid.
    invalid_parameter = 12,      ///< Another numeric or enum parameter is invalid.
    unbracketed_volatility = 13, ///< Volatility bounds do not bracket the observed price.
    solver_non_convergence = 14, ///< A numerical solver exhausted its iterations.
    solver_non_finite = 15,      ///< A numerical solver encountered a non-finite value.
    unbracketed_coupon = 16,     ///< Coupon bounds do not bracket the observed price.
    backend_unavailable = 17,    ///< The requested pricing backend is not available.
    backend_failure = 18,        ///< The selected backend failed during pricing.
    unsupported_operation = 19,  ///< The operation is not supported for the supplied inputs.
};

/// Describes a failed domain or operational result.
struct Error {
    ErrorCategory category; ///< Stable machine-readable failure category.
    std::string message;    ///< Human-readable diagnostic; callers must not parse it.

    /// Compares the category and diagnostic message.
    friend bool operator==(const Error&, const Error&) = default;
};

/// Result of a public operation, containing either `T` or an Error.
template <typename T>
using Result = std::expected<T, Error>;

} // namespace kiyosi
