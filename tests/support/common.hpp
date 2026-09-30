#pragma once

#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi::test {

constexpr kiyosi::Date day(int year, unsigned month, unsigned day_number)
{
    return kiyosi::Date{std::chrono::year{year} / std::chrono::month{month} / std::chrono::day{day_number}};
}

inline double greek_value(const kiyosi::PricingResult& result, kiyosi::Greek measure)
{
    const auto value = result.require(measure);
    REQUIRE(value.has_value());
    return *value;
}

} // namespace kiyosi::test
