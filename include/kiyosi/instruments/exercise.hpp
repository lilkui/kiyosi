#pragma once

#include <concepts>
#include <type_traits>

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

} // namespace kiyosi
