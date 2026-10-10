<!-- Release Pipeline icon - 64x64
     1:8 等比缩放。描边与圆角对小尺寸做过加粗/收紧补偿，
     阶段 1 提到 50% 透明度以免在 64px 下消失。 -->
<Snapshot type="png" background="transparent">
    <Container
        width="64" height="64"
        borderRadius="14.5"
        gradientType="LINEAR"
        gradientColors="#0B1B4D,#1E3A8A,#5B21B6"
        gradientStops="0,0.55,1"
        gradientBegin="TOP_LEFT"
        gradientEnd="BOTTOM_RIGHT">

        <Stack>

            <!-- 阶段 1：已完成 -->
            <Positioned left="13" bottom="12" width="9.5" height="11">
                <Container color="#38BDF880" borderRadius="3"/>
            </Positioned>

            <!-- 阶段 2：进行中 -->
            <Positioned left="27.25" bottom="12" width="9.5" height="17.5">
                <Container color="#22D3EEE0" borderRadius="3"/>
            </Positioned>

            <!-- 阶段 3：发布中 -->
            <Positioned left="41.5" bottom="12" width="9.5" height="24">
                <Container color="#67E8F9" borderRadius="3"/>
            </Positioned>

            <!-- 箭头横翼 -->
            <Positioned left="36.5" top="9.875" width="13" height="4.5">
                <Container color="#F0F9FF" borderRadius="2.25"/>
            </Positioned>

            <!-- 箭头竖翼 -->
            <Positioned left="47.375" top="11" width="4.5" height="13">
                <Container color="#F0F9FF" borderRadius="2.25"/>
            </Positioned>

            <!-- 箭杆：旋转 45° -->
            <Positioned left="42.6" top="10.025" width="4.5" height="13.5">
                <Transform
                    matrix="(0.7071,0.7071,0,0,-0.7071,0.7071,0,0,0,0,1,0,0,0,0,1)"
                    alignment="CENTER">
                    <Container width="4.5" height="13.5" color="#F0F9FF" borderRadius="2.25"/>
                </Transform>
            </Positioned>

        </Stack>
    </Container>
</Snapshot>