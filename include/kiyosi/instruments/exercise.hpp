#pragma once

#include <concepts>
#include <type_traits>
#include <utility>
#include <vector>

#include <kiyosi/core/time.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

template <typename Value>
concept OptionExercise = std::copy_constructible<std::remove_cvref_t<Value>> &&
                         std::equality_comparable<std::remove_cvref_t<Value>>;

struct EuropeanExercise {
    friend bool operator==(const EuropeanExercise&, const EuropeanExercise&) = default;
};

struct AmericanExercise {
    friend bool operator==(const AmericanExercise&, const AmericanExercise&) = default;
};

class BermudanExercise;

namespace detail {
[[nodiscard]] result<BermudanExercise> make_bermudan_exercise(std::vector<date>, date);
}

class BermudanExercise {
public:
    const std::vector<date>& dates() const noexcept { return dates_; }
    std::size_t size() const noexcept { return dates_.size(); }
    bool empty() const noexcept { return dates_.empty(); }
    friend bool operator==(const BermudanExercise&, const BermudanExercise&) = default;

private:
    explicit BermudanExercise(std::vector<date> dates) : dates_(std::move(dates)) {}
    std::vector<date> dates_;
    friend result<BermudanExercise> detail::make_bermudan_exercise(std::vector<date>, date);
};

[[nodiscard]] inline result<BermudanExercise> detail::make_bermudan_exercise(
    std::vector<date> dates, date expiry)
{
    if (dates.empty())
        return std::unexpected(Error{error_category::invalid_schedule,
                                     "Bermudan exercise requires at least one date"});
    auto valid = validate_date_schedule(dates, dates.front(), expiry);
    if (!valid)
        return std::unexpected(Error{error_category::invalid_schedule, valid.error().message});
    return BermudanExercise{std::move(dates)};
}

} // namespace kiyosi
