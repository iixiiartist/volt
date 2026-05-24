def plus_one(digits):
    for i in range(len(digits) - 1, -1, -1):
        if digits[i] == 9:
            digits[i] = 0
        else:
            digits[i] += 1
            return digits
    return [1] + digits

print(plus_one([1,2,3]))
print(plus_one([4,3,2,1]))
print(plus_one([9]))