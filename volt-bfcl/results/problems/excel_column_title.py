def convert_to_title(columnNumber):
    result = ""
    while columnNumber > 0:
        columnNumber -= 1
        result = chr(65 + columnNumber % 26) + result
        columnNumber //= 26
    return result

print(convert_to_title(1))
print(convert_to_title(28))
print(convert_to_title(701))