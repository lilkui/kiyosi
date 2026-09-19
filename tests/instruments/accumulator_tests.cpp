#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <type_traits>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Accumulator factory rejects invalid contracts")
{
    static_assert(!std::is_constructible_v<kiyosi::Accumulator, double, double, double, double, double,
                                           kiyosi::Date, kiyosi::Date>);

    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto terms = kiyosi::AccumulatorTerms{.strike = 100.0,
                                                .knock_out_level = 110.0,
                                                .daily_quantity = 1.0,
                                                .acceleration_factor = 2.0,
                                                .accumulated_quantity = 0.0,
                                                .effective_date = effective_date,
                                                .expiry_date = expiry_date};
    auto invalid = terms;
    invalid.strike = 0.0;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::ErrorCategory::invalid_strike,
                         "strike must be finite and positive"}));
    invalid = terms;
    invalid.knock_out_level = 0.0;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::ErrorCategory::invalid_parameter,
                         "knock-out level must be finite and positive"}));
    invalid = terms;
    invalid.daily_quantity = -1.0;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::ErrorCategory::invalid_parameter,
                         "daily quantity must be finite and non-negative"}));
    invalid = terms;
    invalid.acceleration_factor = -1.0;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::ErrorCategory::invalid_parameter,
                         "acceleration factor must be finite and non-negative"}));
    invalid = terms;
    invalid.accumulated_quantity = -1.0;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::ErrorCategory::invalid_parameter,
                         "accumulated quantity must be finite and non-negative"}));
    invalid = terms;
    invalid.effective_date = expiry_date;
    invalid.expiry_date = effective_date;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::ErrorCategory::invalid_expiry,
                         "expiry date must not precede the effective date"}));
}

TEST_CASE("Accumulator exposes its contractual terms")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto accumulator = kiyosi::make_accumulator({.strike = 100.0,
                                                       .knock_out_level = 110.0,
                                                       .daily_quantity = 1.0,
                                                       .acceleration_factor = 2.0,
                                                       .accumulated_quantity = 3.0,
                                                       .effective_date = effective_date,
                                                       .expiry_date = expiry_date});
    REQUIRE(accumulator.has_value());
    CHECK(accumulator->strike() == 100.0);
    CHECK(accumulator->knock_out_level() == 110.0);
    CHECK(accumulator->daily_quantity() == 1.0);
    CHECK(accumulator->acceleration_factor() == 2.0);
    CHECK(accumulator->accumulated_quantity() == 3.0);
    CHECK(accumulator->effective_date() == effective_date);
    CHECK(accumulator->expiry_date() == expiry_date);
}
