#pragma once

#include <chrono>
#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi::test {

inline kiyosi::date day(int year, unsigned month, unsigned day_number)
{
    return kiyosi::date{std::chrono::year{year} / std::chrono::month{month} / std::chrono::day{day_number}};
}

inline double risk_value(const kiyosi::PricingResult& result, kiyosi::risk_measure measure)
{
    return *result.require(measure);
}

}
