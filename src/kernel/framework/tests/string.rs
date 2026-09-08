use crate::kernel::framework::lib::string::{
    memchr, memcpy, memmove, memset, safe_memcmp, safe_memcpy, safe_memset, secure_zero, strcat,
    strchr, strcmp, strcpy, strlen, strncmp, strncpy, strrchr, strstr,
};
use crate::kernel::framework::tests::{TestResult, assert_eq_test, check, runner};
use crate::register_tests_inner;

fn strlen_basic() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        assert_eq_test!(strlen(c"Hello".as_ptr() as *const i8), 5, "strlen Hello");
        assert_eq_test!(strlen(c"".as_ptr() as *const i8), 0, "strlen empty");
        assert_eq_test!(
            strlen(c"A longer test string".as_ptr() as *const i8),
            20,
            "strlen long"
        );
    }
    TestResult::Pass
}

fn strcmp_operations() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        assert_eq_test!(
            strcmp(c"test".as_ptr() as *const i8, c"test".as_ptr() as *const i8),
            0,
            "equal"
        );
        check!(
            strcmp(c"abc".as_ptr() as *const i8, c"abd".as_ptr() as *const i8) < 0,
            "less than"
        );
        check!(
            strcmp(c"xyz".as_ptr() as *const i8, c"xya".as_ptr() as *const i8) > 0,
            "greater than"
        );
    }
    TestResult::Pass
}

fn strncmp_limit() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        assert_eq_test!(
            strncmp(
                c"abcdef".as_ptr() as *const i8,
                c"abcxyz".as_ptr() as *const i8,
                3
            ),
            0,
            "first 3 equal"
        );
        check!(
            strncmp(
                c"abcdef".as_ptr() as *const i8,
                c"abcxyz".as_ptr() as *const i8,
                4
            ) < 0,
            "first 4 differ"
        );
    }
    TestResult::Pass
}

fn strcpy_and_strncpy() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let mut buffer = [0i8; 20];
        strcpy(buffer.as_mut_ptr(), c"Hello World".as_ptr() as *const i8);
        assert_eq_test!(strlen(buffer.as_ptr()), 11, "strcpy len");
        let mut buffer2 = [0i8; 10];
        strncpy(buffer2.as_mut_ptr(), c"Testing".as_ptr() as *const i8, 5);
        assert_eq_test!(strlen(buffer2.as_ptr()), 5, "strncpy len");
        strncpy(buffer2.as_mut_ptr(), c"Hi".as_ptr() as *const i8, 5);
        assert_eq_test!(buffer2[2], 0, "strncpy padding");
    }
    TestResult::Pass
}

fn strcat_basic() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let mut buffer = [0i8; 30];
        strcpy(buffer.as_mut_ptr(), c"Hello ".as_ptr() as *const i8);
        strcat(buffer.as_mut_ptr(), c"World!".as_ptr() as *const i8);
        assert_eq_test!(strlen(buffer.as_ptr()), 12, "strcat len");
    }
    TestResult::Pass
}

fn strchr_and_strrchr() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let s = b"Hello World\0";
        let result = strchr(s.as_ptr() as *const i8, 'o' as i32);
        check!(!result.is_null(), "strchr found");
        assert_eq_test!(*result, 'o' as i8, "strchr value");
        let result = strrchr(s.as_ptr() as *const i8, 'o' as i32);
        check!(!result.is_null(), "strrchr found");
    }
    TestResult::Pass
}

fn strstr_basic() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let haystack = b"The quick brown fox jumps over the lazy dog\0";
        let result = strstr(
            haystack.as_ptr() as *const i8,
            c"brown fox".as_ptr() as *const i8,
        );
        check!(!result.is_null(), "strstr found");
        let result = strstr(haystack.as_ptr() as *const i8, c"cat".as_ptr() as *const i8);
        check!(result.is_null(), "strstr not found");
        let result = strstr(haystack.as_ptr() as *const i8, c"".as_ptr() as *const i8);
        check!(!result.is_null(), "strstr empty returns haystack");
    }
    TestResult::Pass
}

fn memcpy_and_memmove() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let mut dest = [0u8; 10];
        let src = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        memcpy(dest.as_mut_ptr(), src.as_ptr(), 10);
        assert_eq_test!(dest, src, "memcpy result");
        let mut overlap = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        memmove(overlap.as_mut_ptr(), overlap.as_ptr().add(2), 8);
        assert_eq_test!(
            overlap,
            [3u8, 4, 5, 6, 7, 8, 9, 10, 9, 10],
            "memmove overlap"
        );
    }
    TestResult::Pass
}

fn memset_operations() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let mut buffer = [0xABu8; 20];
        memset(buffer.as_mut_ptr(), 0x00, 20);
        assert_eq_test!(buffer, [0u8; 20], "memset zero");
        memset(buffer.as_mut_ptr(), 0xFF, 10);
        for i in 0..10 {
            assert_eq_test!(buffer[i], 0xFF, "memset FF");
        }
        for i in 10..20 {
            assert_eq_test!(buffer[i], 0x00, "memset untouched");
        }
    }
    TestResult::Pass
}

fn memcmp_basic() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let a = [1u8, 2, 3, 4, 5];
        let b = [1u8, 2, 3, 4, 5];
        let c = [1u8, 2, 3, 4, 6];
        assert_eq_test!(safe_memcmp(&a, &b), core::cmp::Ordering::Equal, "equal");
        check!(
            safe_memcmp(&a, &c) == core::cmp::Ordering::Less,
            "less than"
        );
    }
    TestResult::Pass
}

fn memchr_basic() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let data = [1, 2, 3, 4, 5, 3, 7, 8];
        let result = memchr(data.as_ptr(), 3, 8);
        check!(!result.is_null(), "memchr found");
        let offset = result.cast_const().offset_from(data.as_ptr()) as usize;
        assert_eq_test!(offset, 2, "memchr offset");
        let result = memchr(data.as_ptr(), 9, 8);
        check!(result.is_null(), "memchr not found");
    }
    TestResult::Pass
}

fn secure_zero_basic() -> TestResult {
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        let mut secret = [0xDEu8, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
        secure_zero(secret.as_mut_ptr(), 6);
        for byte in &secret {
            assert_eq_test!(*byte, 0, "secure zero");
        }
    }
    TestResult::Pass
}

fn safe_interfaces() -> TestResult {
    let mut dest = [0u8; 10];
    let src = [1, 2, 3, 4, 5];
    let copied = safe_memcpy(&mut dest, &src);
    assert_eq_test!(copied, 5, "safe_memcpy count");
    assert_eq_test!(&dest[..5], &src[..], "safe_memcpy data");
    safe_memset(&mut dest, 0xFF, None);
    assert_eq_test!(dest, [0xFF; 10], "safe_memset");
    assert_eq_test!(
        safe_memcmp(&[1, 2, 3], &[1, 2, 3]),
        core::cmp::Ordering::Equal,
        "safe_memcmp equal"
    );
    assert_eq_test!(
        safe_memcmp(&[1, 2, 3], &[1, 2, 4]),
        core::cmp::Ordering::Less,
        "safe_memcmp less"
    );
    TestResult::Pass
}

pub fn register_string_tests() {
    let r = runner();
    register_tests_inner! { r:
        "lib::string": {
            "strlen": strlen_basic,
            "strcmp": strcmp_operations,
            "strncmp": strncmp_limit,
            "strcpy_strncpy": strcpy_and_strncpy,
            "strcat": strcat_basic,
            "strchr_strrchr": strchr_and_strrchr,
            "strstr": strstr_basic,
            "memcpy_memmove": memcpy_and_memmove,
            "memset": memset_operations,
            "memcmp": memcmp_basic,
            "memchr": memchr_basic,
            "secure_zero": secure_zero_basic,
            "safe_interfaces": safe_interfaces,
        },
    }
}

pub fn register_tests() {
    register_string_tests();
}
