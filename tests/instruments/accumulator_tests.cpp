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
                                           kiyosi::date, kiyosi::date>);

    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto terms = kiyosi::AccumulatorTerms{.strike = 100.0,
                                                .knock_out = 110.0,
                                                .daily_quantity = 1.0,
                                                .acceleration = 2.0,
                                                .accumulated_quantity = 0.0,
                                                .effective = effective,
                                                .expiry = expiry};
    auto invalid = terms;
    invalid.strike = 0.0;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::error_category::invalid_strike,
                         "strike must be finite and positive"}));
    invalid = terms;
    invalid.knock_out = 0.0;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::error_category::invalid_parameter,
                         "knock-out price must be finite and positive"}));
    invalid = terms;
    invalid.daily_quantity = -1.0;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::error_category::invalid_parameter,
                         "daily quantity must be finite and non-negative"}));
    invalid = terms;
    invalid.acceleration = -1.0;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::error_category::invalid_parameter,
                         "acceleration must be finite and non-negative"}));
    invalid = terms;
    invalid.accumulated_quantity = -1.0;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::error_category::invalid_parameter,
                         "accumulated quantity must be finite and non-negative"}));
    invalid = terms;
    invalid.effective = expiry;
    invalid.expiry = effective;
    CHECK((kiyosi::make_accumulator(invalid).error() ==
           kiyosi::Error{kiyosi::error_category::invalid_expiry,
                         "expiry must not precede effective"}));
}

TEST_CASE("Accumulator exposes its contractual terms")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto accumulator = kiyosi::make_accumulator({.strike = 100.0,
                                                       .knock_out = 110.0,
                                                       .daily_quantity = 1.0,
                                                       .acceleration = 2.0,
                                                       .accumulated_quantity = 3.0,
                                                       .effective = effective,
                                                       .expiry = expiry});
    REQUIRE(accumulator.has_value());
    CHECK(accumulator->strike() == 100.0);
    CHECK(accumulator->knock_out() == 110.0);
    CHECK(accumulator->daily_quantity() == 1.0);
    CHECK(accumulator->acceleration() == 2.0);
    CHECK(accumulator->accumulated_quantity() == 3.0);
    CHECK(accumulator->effective() == effective);
    CHECK(accumulator->expiry() == expiry);
}
