#pragma once

#include <algorithm>
#include <array>
#include <cmath>

namespace kiyosi::detail {

using QuadraticRegressionMatrix = std::array<std::array<double, 4>, 3>;

inline bool solve_quadratic(QuadraticRegressionMatrix matrix,
                            std::array<double, 3>& coefficients)
{
    for (int column = 0; column < 3; ++column) {
        int pivot = column;
        for (int row = column + 1; row < 3; ++row)
            if (std::abs(matrix[row][column]) > std::abs(matrix[pivot][column])) pivot = row;
        if (!std::isfinite(matrix[pivot][column]) ||
            std::abs(matrix[pivot][column]) <=
                1e-14 * std::max(1.0, std::abs(matrix[pivot][3])))
            return false;
        std::swap(matrix[column], matrix[pivot]);
        for (int row = column + 1; row < 3; ++row) {
            const double factor = matrix[row][column] / matrix[column][column];
            for (int entry = column; entry <= 3; ++entry)
                matrix[row][entry] -= factor * matrix[column][entry];
        }
    }
    for (int row = 2; row >= 0; --row) {
        double value = matrix[row][3];
        for (int column = row + 1; column < 3; ++column)
            value -= matrix[row][column] * coefficients[column];
        coefficients[row] = value / matrix[row][row];
    }
    return std::all_of(coefficients.begin(), coefficients.end(),
                       [](double value) { return std::isfinite(value); });
}

} // namespace kiyosi::detail
