<!-- Release Pipeline icon - 128x128
     1:4 等比缩放。沿用与 512 主图完全相同的箭头构造法
     （竖条旋转 45° 作箭杆，水平/竖直圆角条作两翼）。 -->
<Snapshot type="png" background="transparent">
    <Container
        width="128" height="128"
        borderRadius="29"
        gradientType="LINEAR"
        gradientColors="#0B1B4D,#1E3A8A,#5B21B6"
        gradientStops="0,0.55,1"
        gradientBegin="TOP_LEFT"
        gradientEnd="BOTTOM_RIGHT"
        border="1 SOLID #FFFFFF33">

        <Stack>

            <!-- 阶段 1：已完成 -->
            <Positioned left="26" bottom="24" width="19" height="22">
                <Container color="#38BDF870" borderRadius="5.5"/>
            </Positioned>

            <!-- 阶段 2：进行中 -->
            <Positioned left="54.5" bottom="24" width="19" height="35">
                <Container color="#22D3EED0" borderRadius="5.5"/>
            </Positioned>

            <!-- 阶段 3：发布中 -->
            <Positioned left="83" bottom="24" width="19" height="48">
                <Container color="#67E8F9" borderRadius="5.5"/>
            </Positioned>

            <!-- 箭头横翼 -->
            <Positioned left="73" top="19.75" width="26" height="8.5">
                <Container color="#F0F9FF" borderRadius="4.25"/>
            </Positioned>

            <!-- 箭头竖翼 -->
            <Positioned left="94.75" top="22" width="8.5" height="26">
                <Container color="#F0F9FF" borderRadius="4.25"/>
            </Positioned>

            <!-- 箭杆：旋转 45°，上端落在尖端 -->
            <Positioned left="85.2" top="20.05" width="8.5" height="27">
                <Transform
                    matrix="(0.7071,0.7071,0,0,-0.7071,0.7071,0,0,0,0,1,0,0,0,0,1)"
                    alignment="CENTER">
                    <Container width="8.5" height="27" color="#F0F9FF" borderRadius="4.25"/>
                </Transform>
            </Positioned>

        </Stack>
    </Container>
</Snapshot>