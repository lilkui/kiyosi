#pragma once

#include <expected>
#include <string>

namespace kiyosi {

enum class error_category : unsigned char {
    invalid_option = 1,
    invalid_strike = 2,
    invalid_volatility = 3,
    invalid_rate = 4,
    invalid_dividend = 5,
    invalid_asset_price = 6,
    invalid_date = 7,
    invalid_expiry = 8,
    invalid_result = 9,
    invalid_schedule = 10,
    invalid_calendar = 11,
    invalid_parameter = 12,
    invalid_quote = 13,
    unbracketed_volatility = 14,
    solver_non_convergence = 15,
    solver_non_finite = 16,
    unbracketed_coupon = 17,
};

struct Error {
    error_category category;
    std::string message;

    friend bool operator==(const Error&, const Error&) = default;
};

template <typename T>
using result = std::expected<T, Error>;

} // namespace kiyosi
