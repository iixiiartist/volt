def is_happy(n):
    seen = set()
    while n != 1 and n not in seen:
        seen.add(n)
        n = sum(int(i) ** 2 for i in str(n))
    return n == 1

print(is_happy(19))
print(is_happy(2))