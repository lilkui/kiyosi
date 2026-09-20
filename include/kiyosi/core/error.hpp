#pragma once

#include <expected>
#include <string>

namespace kiyosi {

enum class ErrorCategory : unsigned char {
    invalid_option = 1,
    invalid_strike = 2,
    invalid_volatility = 3,
    invalid_risk_free_rate = 4,
    invalid_dividend_yield = 5,
    invalid_spot_price = 6,
    invalid_date = 7,
    invalid_time_range = 8,
    invalid_result = 9,
    invalid_schedule = 10,
    invalid_calendar = 11,
    invalid_parameter = 12,
    unbracketed_volatility = 13,
    solver_non_convergence = 14,
    solver_non_finite = 15,
    unbracketed_coupon = 16,
    backend_unavailable = 17,
    backend_failure = 18,
    unsupported_operation = 19,
};

struct Error {
    ErrorCategory category;
    std::string message;

    friend bool operator==(const Error&, const Error&) = default;
};

template <typename T>
using Result = std::expected<T, Error>;

} // namespace kiyosi
